//! Task 기반 모델 라우팅.
//!
//! 이 레이어의 핵심 아이디어: 호출부는 **모델이 아니라 task를 지정한다.**
//! "이 호출은 `VisionPass1`이다"라고만 말하고, 어떤 provider의 어떤 모델이
//! 그 task를 처리할지는 설정이 정한다. 모델을 바꾸는 일이 코드 수정이 아니라
//! 환경변수 변경이 되고, task 단위로 서로 다른 provider를 섞을 수 있다.
//!
//! ## 환경변수
//!
//! ```text
//! LLM_DEFAULT_PROVIDER=openai          # 명시적 지정이 없는 task의 provider
//! LLM_TASK_VISION_PASS1=anthropic:claude-opus-5   # provider와 모델을 함께
//! LLM_TASK_STYLE_NOTE=gpt-4o                      # 모델만 (provider는 기본값)
//! LLM_TIMEOUT_SECS=60
//! LLM_MAX_RETRIES=2
//! LLM_ANTHROPIC_EFFORT=low
//! ```

use std::collections::HashMap;

use super::provider::ProviderId;

/// LLM을 쓰는 지점 하나하나에 붙는 이름.
///
/// 새 호출 지점을 추가할 때 여기에 variant를 넣으면 자동으로 설정 가능한 대상이 된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlmTask {
    /// 단일 코디 추천 (레거시 경로).
    OutfitRecommendation,
    /// 5개 코디 후보 생성.
    OutfitCandidates,
    /// RAG 없이 이미지 1회 분석 (폴백 경로).
    VisionAnalyze,
    /// 2-Pass RAG의 1단계 — 시각 특징 서술.
    VisionPass1,
    /// 2-Pass RAG의 2단계 — 레퍼런스 참조 정밀 분석.
    VisionPass2,
    /// 규칙 엔진 결과를 자연어로 설명.
    OutfitExplanation,
    /// 채팅 에이전트 (도구 호출 루프).
    ChatAgent,
    /// 착장 Style Note 생성.
    StyleNote,
    /// 생성된 이미지의 성별 자동 검수.
    GenderVerify,
    /// 룩북 이미지 생성.
    ImageGeneration,
    /// 레퍼런스/쿼리 텍스트 임베딩.
    Embedding,
}

impl LlmTask {
    pub const ALL: &'static [LlmTask] = &[
        LlmTask::OutfitRecommendation,
        LlmTask::OutfitCandidates,
        LlmTask::VisionAnalyze,
        LlmTask::VisionPass1,
        LlmTask::VisionPass2,
        LlmTask::OutfitExplanation,
        LlmTask::ChatAgent,
        LlmTask::StyleNote,
        LlmTask::GenderVerify,
        LlmTask::ImageGeneration,
        LlmTask::Embedding,
    ];

    /// 로그 필드에 쓰이는 이름.
    pub fn name(&self) -> &'static str {
        match self {
            LlmTask::OutfitRecommendation => "outfit_recommendation",
            LlmTask::OutfitCandidates => "outfit_candidates",
            LlmTask::VisionAnalyze => "vision_analyze",
            LlmTask::VisionPass1 => "vision_pass1",
            LlmTask::VisionPass2 => "vision_pass2",
            LlmTask::OutfitExplanation => "outfit_explanation",
            LlmTask::ChatAgent => "chat_agent",
            LlmTask::StyleNote => "style_note",
            LlmTask::GenderVerify => "gender_verify",
            LlmTask::ImageGeneration => "image_generation",
            LlmTask::Embedding => "embedding",
        }
    }

    /// `LLM_TASK_` 뒤에 붙는 환경변수 키.
    pub fn env_key(&self) -> String {
        format!("LLM_TASK_{}", self.name().to_ascii_uppercase())
    }
}

/// task 하나의 실행 설정.
#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub provider: ProviderId,
    pub model: String,
    /// 샘플링 온도. `None`이면 provider 기본값.
    /// Anthropic 최신 모델은 이 파라미터를 아예 받지 않는다 (아래 provider 구현 참고).
    pub temperature: Option<f32>,
    pub max_tokens: u32,
}

/// 코드에 박힌 기본값. 환경변수가 없으면 이 값이 쓰인다.
/// 기존 하드코딩 동작과 동일하게 맞춰 두어, 설정 없이 돌려도 거동이 바뀌지 않는다.
fn default_task_config(task: LlmTask) -> TaskConfig {
    let (model, temperature, max_tokens) = match task {
        LlmTask::OutfitRecommendation => ("gpt-4o-mini", Some(0.4), 1000),
        LlmTask::OutfitCandidates => ("gpt-4o-mini", Some(0.5), 3000),
        LlmTask::VisionAnalyze => ("gpt-4o-mini", Some(0.3), 500),
        LlmTask::VisionPass1 => ("gpt-4o-mini", Some(0.3), 500),
        LlmTask::VisionPass2 => ("gpt-4o-mini", Some(0.2), 500),
        LlmTask::OutfitExplanation => ("gpt-4o-mini", Some(0.5), 500),
        LlmTask::ChatAgent => ("gpt-4o-mini", Some(0.5), 1000),
        LlmTask::StyleNote => ("gpt-4o-mini", Some(0.7), 300),
        LlmTask::GenderVerify => ("gpt-4o-mini", None, 5),
        LlmTask::ImageGeneration => ("gpt-image-2", None, 0),
        LlmTask::Embedding => ("text-embedding-3-small", None, 0),
    };

    TaskConfig {
        provider: ProviderId::OpenAi,
        model: model.to_string(),
        temperature,
        max_tokens,
    }
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    tasks: HashMap<LlmTask, TaskConfig>,
    pub timeout_secs: u64,
    pub max_retries: u32,
    /// Anthropic `output_config.effort`. 이 프로젝트의 task는 대부분 기계적인
    /// 추출/분류라서 낮은 effort가 적합하다.
    pub anthropic_effort: String,
}

impl LlmConfig {
    pub fn from_env() -> Self {
        let default_provider = std::env::var("LLM_DEFAULT_PROVIDER")
            .ok()
            .and_then(|s| ProviderId::parse(&s))
            .unwrap_or(ProviderId::OpenAi);

        let mut tasks = HashMap::new();
        for &task in LlmTask::ALL {
            let mut cfg = default_task_config(task);
            cfg.provider = default_provider;

            if let Ok(raw) = std::env::var(task.env_key()) {
                match parse_task_override(&raw) {
                    Some((provider, model)) => {
                        if let Some(p) = provider {
                            cfg.provider = p;
                        }
                        cfg.model = model;
                        tracing::info!(
                            task = task.name(),
                            provider = cfg.provider.as_str(),
                            model = %cfg.model,
                            "LLM task 라우팅 재정의"
                        );
                    }
                    None => {
                        tracing::warn!(
                            task = task.name(),
                            raw = %raw,
                            "{} 값을 해석할 수 없어 기본값을 사용합니다 (형식: 'provider:model' 또는 'model')",
                            task.env_key()
                        );
                    }
                }
            }

            tasks.insert(task, cfg);
        }

        Self {
            tasks,
            timeout_secs: env_parse("LLM_TIMEOUT_SECS", 60),
            max_retries: env_parse("LLM_MAX_RETRIES", 2),
            anthropic_effort: std::env::var("LLM_ANTHROPIC_EFFORT")
                .unwrap_or_else(|_| "low".to_string()),
        }
    }

    pub fn task(&self, task: LlmTask) -> &TaskConfig {
        // ALL을 순회하며 채우므로 모든 task가 존재한다.
        self.tasks
            .get(&task)
            .expect("모든 LlmTask는 from_env에서 채워진다")
    }

    /// 설정에 실제로 등장하는 provider 집합. 필요한 클라이언트만 만들기 위해 쓴다.
    pub fn providers_in_use(&self) -> Vec<ProviderId> {
        let mut seen: Vec<ProviderId> = Vec::new();
        for cfg in self.tasks.values() {
            if !seen.contains(&cfg.provider) {
                seen.push(cfg.provider);
            }
        }
        seen
    }
}

/// `"anthropic:claude-opus-5"` → `(Some(Anthropic), "claude-opus-5")`
/// `"gpt-4o"` → `(None, "gpt-4o")`
fn parse_task_override(raw: &str) -> Option<(Option<ProviderId>, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.split_once(':') {
        Some((provider, model)) => {
            let provider = ProviderId::parse(provider)?;
            let model = model.trim();
            if model.is_empty() {
                return None;
            }
            Some((Some(provider), model.to_string()))
        }
        None => Some((None, raw.to_string())),
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_and_model() {
        let (p, m) = parse_task_override("anthropic:claude-opus-5").unwrap();
        assert_eq!(p, Some(ProviderId::Anthropic));
        assert_eq!(m, "claude-opus-5");
    }

    #[test]
    fn parses_bare_model() {
        let (p, m) = parse_task_override("gpt-4o").unwrap();
        assert_eq!(p, None);
        assert_eq!(m, "gpt-4o");
    }

    #[test]
    fn rejects_unknown_provider_and_empty_model() {
        assert!(parse_task_override("cohere:command-r").is_none());
        assert!(parse_task_override("openai:").is_none());
        assert!(parse_task_override("   ").is_none());
    }

    #[test]
    fn every_task_has_a_default() {
        for &task in LlmTask::ALL {
            let cfg = default_task_config(task);
            assert!(
                !cfg.model.is_empty(),
                "{} has no default model",
                task.name()
            );
        }
    }

    /// 추상화 도입 전 각 호출부에 하드코딩돼 있던 값과 동일해야 한다.
    /// 설정 없이 서버를 띄웠을 때 거동이 바뀌지 않는다는 보장.
    #[test]
    fn defaults_match_pre_refactor_hardcoded_values() {
        let expected = [
            (
                LlmTask::OutfitRecommendation,
                "gpt-4o-mini",
                Some(0.4),
                1000,
            ),
            (LlmTask::OutfitCandidates, "gpt-4o-mini", Some(0.5), 3000),
            (LlmTask::VisionAnalyze, "gpt-4o-mini", Some(0.3), 500),
            (LlmTask::VisionPass1, "gpt-4o-mini", Some(0.3), 500),
            (LlmTask::VisionPass2, "gpt-4o-mini", Some(0.2), 500),
            (LlmTask::OutfitExplanation, "gpt-4o-mini", Some(0.5), 500),
            (LlmTask::ChatAgent, "gpt-4o-mini", Some(0.5), 1000),
            (LlmTask::StyleNote, "gpt-4o-mini", Some(0.7), 300),
            (LlmTask::GenderVerify, "gpt-4o-mini", None, 5),
            (LlmTask::ImageGeneration, "gpt-image-2", None, 0),
            (LlmTask::Embedding, "text-embedding-3-small", None, 0),
        ];

        for (task, model, temperature, max_tokens) in expected {
            let cfg = default_task_config(task);
            assert_eq!(cfg.provider, ProviderId::OpenAi, "{}", task.name());
            assert_eq!(cfg.model, model, "{}", task.name());
            assert_eq!(cfg.temperature, temperature, "{}", task.name());
            assert_eq!(cfg.max_tokens, max_tokens, "{}", task.name());
        }
    }

    #[test]
    fn env_keys_are_unique() {
        let mut keys: Vec<String> = LlmTask::ALL.iter().map(|t| t.env_key()).collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "task env keys must be unique");
    }
}
