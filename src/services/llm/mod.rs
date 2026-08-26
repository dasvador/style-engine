//! LLM provider 추상화 레이어.
//!
//! 애플리케이션의 모든 모델 호출은 이 모듈 하나를 지나간다. 목적은 세 가지다.
//!
//! 1. **교체 가능성** — 호출부는 모델명을 모른다. task 이름만 지정하고, 어떤 provider의
//!    어떤 모델이 처리할지는 설정이 정한다 ([`config`] 참고).
//! 2. **능력 차이 흡수** — provider마다 JSON 모드, 도구 호출 형식, 샘플링 파라미터 지원이
//!    다르다. 그 차이는 provider 구현체가 삼키고, 호출부는 동일한 계약만 본다.
//! 3. **단일 계측 지점** — 모든 호출이 한 경로를 지나므로 재시도·타임아웃·토큰/비용
//!    계측을 여기 한 번만 구현하면 된다 ([`usage`] 참고).
//!
//! ```ignore
//! // JSON 응답: 파싱까지 재시도 루프 안에서 처리된다.
//! let parsed: Pass1Result = state.llm.chat_json(
//!     LlmTask::VisionPass1,
//!     ChatRequest::new(vec![Message::user_image(prompt, image_data_url)])
//!         .system(SYSTEM_PROMPT)
//!         .json(),
//! ).await?;
//!
//! // 텍스트/도구 호출 응답.
//! let resp = state.llm.chat(LlmTask::ChatAgent, req).await?;
//! ```

pub mod anthropic;
pub mod config;
pub mod error;
pub mod openai;
pub mod provider;
pub mod types;
pub mod usage;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use config::{LlmConfig, LlmTask, TaskConfig};
pub use error::LlmError;
pub use provider::ProviderId;
// 모듈의 공개 표면. bin 타깃(main.rs)은 이 중 일부만 쓰지만, 나머지도 의도적으로 공개한다.
#[allow(unused_imports)]
pub use types::{
    ChatRequest, ChatResponse, ContentPart, EmbeddingResult, ImageRequest, ImageResult, Message,
    ResponseFormat, StopReason, ToolCall, ToolDef,
};
pub use usage::Usage;

use provider::{ChatProvider, EmbeddingProvider, ImageProvider};

/// 재시도 간 대기 시간의 기준값. attempt마다 2배로 늘어난다.
const RETRY_BASE_DELAY_MS: u64 = 250;

pub struct LlmClient {
    config: LlmConfig,
    chat_providers: HashMap<ProviderId, Arc<dyn ChatProvider>>,
    embedding_providers: HashMap<ProviderId, Arc<dyn EmbeddingProvider>>,
    image_providers: HashMap<ProviderId, Arc<dyn ImageProvider>>,
    /// API 키가 실제로 주어진 provider들.
    configured: HashSet<ProviderId>,
}

impl LlmClient {
    pub fn from_env(http: reqwest::Client) -> Self {
        let config = LlmConfig::from_env();

        let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

        let mut chat_providers: HashMap<ProviderId, Arc<dyn ChatProvider>> = HashMap::new();
        let mut embedding_providers: HashMap<ProviderId, Arc<dyn EmbeddingProvider>> =
            HashMap::new();
        let mut image_providers: HashMap<ProviderId, Arc<dyn ImageProvider>> = HashMap::new();
        let mut configured = HashSet::new();

        // OpenAI — chat / embedding / image 전부 제공.
        // 레지스트리 키는 provider가 스스로 신고한 id를 쓴다. 여기에 상수를 손으로 적으면
        // 엉뚱한 키에 등록해도 컴파일이 통과하고, 라우팅이 조용히 잘못된다.
        let openai = Arc::new(openai::OpenAiProvider::new(
            http.clone(),
            openai_key.clone(),
        ));
        chat_providers.insert(ChatProvider::id(openai.as_ref()), openai.clone());
        embedding_providers.insert(EmbeddingProvider::id(openai.as_ref()), openai.clone());
        image_providers.insert(ImageProvider::id(openai.as_ref()), openai);
        if !openai_key.is_empty() && openai_key != "sk-your-key-here" {
            configured.insert(ProviderId::OpenAi);
        }

        // Anthropic — chat/vision만 제공. 임베딩·이미지 생성 엔드포인트가 없으므로
        // 해당 레지스트리에 등록하지 않는다. 그 task를 Anthropic으로 라우팅하면
        // 런타임에 Unsupported 에러로 즉시 드러난다.
        let anthropic = Arc::new(anthropic::AnthropicProvider::new(
            http,
            anthropic_key.clone(),
            config.anthropic_effort.clone(),
        ));
        chat_providers.insert(anthropic.id(), anthropic);
        if !anthropic_key.is_empty() {
            configured.insert(ProviderId::Anthropic);
        }

        let client = Self {
            config,
            chat_providers,
            embedding_providers,
            image_providers,
            configured,
        };
        client.log_routing_table();
        client
    }

    /// 기동 시 어떤 task가 어디로 가는지 한 번 찍어둔다.
    /// 모델 교체가 설정으로 이뤄지는 만큼, 지금 뜬 서버가 무엇을 쓰는지 로그로 확인할 수 있어야 한다.
    fn log_routing_table(&self) {
        for &task in LlmTask::ALL {
            let cfg = self.config.task(task);
            tracing::info!(
                task = task.name(),
                provider = cfg.provider.as_str(),
                model = %cfg.model,
                configured = self.configured.contains(&cfg.provider),
                "LLM 라우팅"
            );
        }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// 이 task를 지금 실행할 수 있는지 (API 키가 있는지) 확인한다.
    /// 라우트가 요청을 받자마자 400으로 돌려보낼 때 쓴다.
    pub fn ensure_configured(&self, task: LlmTask) -> Result<(), LlmError> {
        let cfg = self.config.task(task);
        if self.configured.contains(&cfg.provider) {
            return Ok(());
        }
        Err(LlmError::Config(format!(
            "{} task가 {}로 라우팅되어 있으나 해당 provider의 API 키가 설정되지 않았습니다",
            task.name(),
            cfg.provider
        )))
    }

    // ─── chat ───

    /// 텍스트/도구 호출 응답을 그대로 돌려준다.
    pub async fn chat(&self, task: LlmTask, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let (cfg, provider, req) = self.prepare_chat(task, req)?;

        self.run_with_retry(task, &cfg, || {
            let (provider, model, req) = (provider.clone(), cfg.model.clone(), &req);
            async move {
                let resp = provider.chat(&model, req).await?;
                let usage = resp.usage;
                Ok((resp, usage))
            }
        })
        .await
    }

    /// JSON 응답을 도메인 타입으로 파싱해서 돌려준다.
    ///
    /// 파싱이 **재시도 루프 안에서** 일어나는 것이 핵심이다. 스키마에 맞지 않는 출력은
    /// 모델이 흔들린 결과지 요청이 잘못된 것이 아니므로, 같은 요청을 다시 보내면 대개 회복된다.
    /// 파싱을 루프 밖에서 하면 [`LlmError::Decode`]를 재시도 대상으로 분류해 둔 의미가 없어진다.
    pub async fn chat_json<T>(&self, task: LlmTask, req: ChatRequest) -> Result<T, LlmError>
    where
        T: serde::de::DeserializeOwned,
    {
        let (cfg, provider, req) = self.prepare_chat(task, req)?;

        self.run_with_retry(task, &cfg, || {
            let (provider, model, req) = (provider.clone(), cfg.model.clone(), &req);
            async move {
                let resp = provider.chat(&model, req).await?;
                let usage = resp.usage;
                let parsed = resp.parse_json::<T>()?;
                Ok((parsed, usage))
            }
        })
        .await
    }

    /// task 설정을 요청에 반영하고 provider를 찾는다.
    fn prepare_chat(
        &self,
        task: LlmTask,
        mut req: ChatRequest,
    ) -> Result<(TaskConfig, Arc<dyn ChatProvider>, ChatRequest), LlmError> {
        let cfg = self.config.task(task).clone();

        // 호출부가 지정하지 않은 모델 파라미터는 설정에서 채운다.
        if req.temperature.is_none() {
            req.temperature = cfg.temperature;
        }
        if req.max_tokens.is_none() {
            req.max_tokens = Some(cfg.max_tokens);
        }

        let provider = self
            .chat_providers
            .get(&cfg.provider)
            .ok_or(LlmError::Unsupported {
                provider: cfg.provider,
                capability: "chat",
            })?
            .clone();

        Ok((cfg, provider, req))
    }

    // ─── embedding ───

    pub async fn embed(&self, inputs: &[String]) -> Result<EmbeddingResult, LlmError> {
        let task = LlmTask::Embedding;
        let cfg = self.config.task(task).clone();

        let provider = self
            .embedding_providers
            .get(&cfg.provider)
            .ok_or(LlmError::Unsupported {
                provider: cfg.provider,
                capability: "embedding",
            })?
            .clone();

        self.run_with_retry(task, &cfg, || {
            let (provider, model) = (provider.clone(), cfg.model.clone());
            async move {
                let r = provider.embed(&model, inputs).await?;
                let usage = r.usage;
                Ok((r, usage))
            }
        })
        .await
    }

    // ─── image ───

    pub async fn generate_image(&self, req: &ImageRequest) -> Result<ImageResult, LlmError> {
        let task = LlmTask::ImageGeneration;
        let cfg = self.config.task(task).clone();

        let provider = self
            .image_providers
            .get(&cfg.provider)
            .ok_or(LlmError::Unsupported {
                provider: cfg.provider,
                capability: "image generation",
            })?
            .clone();

        self.run_with_retry(task, &cfg, || {
            let (provider, model) = (provider.clone(), cfg.model.clone());
            async move {
                let r = provider.generate(&model, req).await?;
                let usage = r.usage;
                Ok((r, usage))
            }
        })
        .await
    }

    /// 타임아웃 + 지수 백오프 재시도 + 계측을 한 곳에서 처리한다.
    ///
    /// provider 구현체는 이 정책을 몰라도 된다 — 한 번의 호출만 정직하게 수행하고,
    /// 실패를 [`LlmError`]로 분류해 주기만 하면 된다.
    ///
    /// `make_future`가 돌려주는 `Usage`는 그 시도에서 실제로 소모된 양이다. 응답 검증까지
    /// 이 안에서 하기 때문에, 검증 실패로 버려진 시도도 비용 로그에 남는다.
    async fn run_with_retry<F, Fut, T>(
        &self,
        task: LlmTask,
        cfg: &TaskConfig,
        make_future: F,
    ) -> Result<T, LlmError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<(T, Usage), LlmError>>,
    {
        let started = Instant::now();
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let mut attempt = 0u32;

        loop {
            attempt += 1;

            let result = match tokio::time::timeout(timeout, make_future()).await {
                Ok(inner) => inner,
                Err(_) => Err(LlmError::Timeout(self.config.timeout_secs)),
            };

            match result {
                Ok((value, usage)) => {
                    usage::record_call(
                        task,
                        cfg.provider,
                        &cfg.model,
                        &usage,
                        started.elapsed(),
                        attempt,
                    );
                    return Ok(value);
                }
                Err(e) => {
                    let can_retry = e.is_retryable() && attempt <= self.config.max_retries;
                    if !can_retry {
                        usage::record_failure(
                            task,
                            cfg.provider,
                            &cfg.model,
                            started.elapsed(),
                            attempt,
                            &e,
                        );
                        return Err(e);
                    }

                    let delay = RETRY_BASE_DELAY_MS * 2u64.pow(attempt - 1);
                    tracing::warn!(
                        task = task.name(),
                        provider = cfg.provider.as_str(),
                        attempt,
                        delay_ms = delay,
                        error = %e,
                        "LLM 호출 실패 — 재시도"
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::llm::types::StopReason;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 처음 `fail_times`번은 스키마에 맞지 않는 본문을, 그 뒤로는 정상 JSON을 돌려준다.
    struct FlakyJsonProvider {
        calls: AtomicU32,
        fail_times: u32,
    }

    #[async_trait::async_trait]
    impl ChatProvider for FlakyJsonProvider {
        fn id(&self) -> ProviderId {
            ProviderId::OpenAi
        }

        async fn chat(&self, _model: &str, _req: &ChatRequest) -> Result<ChatResponse, LlmError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let text = if n < self.fail_times {
                "죄송합니다, 답변을 드릴 수 없습니다." // JSON이 아님
            } else {
                r#"{"value": 42}"#
            };
            Ok(ChatResponse {
                text: Some(text.to_string()),
                tool_calls: Vec::new(),
                stop: StopReason::EndTurn,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                },
            })
        }
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Payload {
        value: u32,
    }

    /// 라우팅 설정과 무관하게 mock이 호출되도록 두 provider 모두에 등록한다.
    fn client_with(provider: Arc<dyn ChatProvider>, max_retries: u32) -> LlmClient {
        let mut config = LlmConfig::from_env();
        config.max_retries = max_retries;
        config.timeout_secs = 5;

        let mut chat_providers: HashMap<ProviderId, Arc<dyn ChatProvider>> = HashMap::new();
        chat_providers.insert(ProviderId::OpenAi, provider.clone());
        chat_providers.insert(ProviderId::Anthropic, provider);

        LlmClient {
            config,
            chat_providers,
            embedding_providers: HashMap::new(),
            image_providers: HashMap::new(),
            configured: HashSet::from([ProviderId::OpenAi, ProviderId::Anthropic]),
        }
    }

    /// A-1의 핵심: 도메인 스키마 파싱 실패가 재시도 루프 **안에서** 일어나야 한다.
    #[tokio::test]
    async fn chat_json_retries_when_body_does_not_match_schema() {
        let provider = Arc::new(FlakyJsonProvider {
            calls: AtomicU32::new(0),
            fail_times: 1,
        });
        let client = client_with(provider.clone(), 2);

        let out: Payload = client
            .chat_json(
                LlmTask::VisionPass1,
                ChatRequest::new(vec![Message::user_text("x")]).json(),
            )
            .await
            .expect("두 번째 시도에서 회복되어야 한다");

        assert_eq!(out, Payload { value: 42 });
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            2,
            "정확히 한 번 재시도해야 한다"
        );
    }

    /// 계속 스키마를 어기면 max_retries+1회까지만 시도하고 포기한다.
    #[tokio::test]
    async fn chat_json_gives_up_after_max_retries() {
        let provider = Arc::new(FlakyJsonProvider {
            calls: AtomicU32::new(0),
            fail_times: u32::MAX,
        });
        let client = client_with(provider.clone(), 2);

        let result: Result<Payload, _> = client
            .chat_json(
                LlmTask::VisionPass1,
                ChatRequest::new(vec![Message::user_text("x")]).json(),
            )
            .await;

        assert!(matches!(result, Err(LlmError::Decode(_))));
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            3,
            "최초 1회 + 재시도 2회"
        );
    }

    /// `chat()`은 파싱하지 않으므로 본문이 JSON이 아니어도 그대로 통과한다.
    #[tokio::test]
    async fn plain_chat_does_not_validate_body() {
        let provider = Arc::new(FlakyJsonProvider {
            calls: AtomicU32::new(0),
            fail_times: u32::MAX,
        });
        let client = client_with(provider.clone(), 2);

        let resp = client
            .chat(
                LlmTask::StyleNote,
                ChatRequest::new(vec![Message::user_text("x")]),
            )
            .await
            .expect("텍스트 응답은 검증 대상이 아니다");

        assert!(resp.text_or_empty().contains("죄송"));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}
