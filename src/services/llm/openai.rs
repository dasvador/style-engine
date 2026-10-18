//! OpenAI provider — Chat Completions / Embeddings / Images.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::error::{ApiErrorDetail, LlmError, ModerationDetails};
use super::provider::{ChatProvider, EmbeddingProvider, ImageProvider, ProviderId};
use super::types::{
    ChatRequest, ChatResponse, ContentPart, EmbeddingResult, ImageEditRequest, ImageRequest,
    ImageResult, Message, ResponseFormat, StopReason, ToolCall, ToolDef,
};
use super::usage::Usage;

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";
const IMAGES_URL: &str = "https://api.openai.com/v1/images/generations";
const IMAGE_EDITS_URL: &str = "https://api.openai.com/v1/images/edits";

pub struct OpenAiProvider {
    http: reqwest::Client,
    api_key: String,
}

impl OpenAiProvider {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }

    fn check_key(&self) -> Result<(), LlmError> {
        if self.api_key.is_empty() || self.api_key == "sk-your-key-here" {
            return Err(LlmError::Config(
                "OPENAI_API_KEY가 설정되지 않았습니다".into(),
            ));
        }
        Ok(())
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value, LlmError> {
        self.check_key()?;

        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let request_id = request_id_header(&resp);
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(status_error(status.as_u16(), request_id, &text));
        }

        serde_json::from_str(&text)
            .map_err(|e| LlmError::Decode(format!("OpenAI 응답 JSON 파싱 실패: {e}")))
    }

    /// 이미지 편집은 JSON 이 아니라 multipart 로 받는다. 응답 처리는 `post` 와 같다.
    async fn post_multipart(
        &self,
        url: &str,
        form: reqwest::multipart::Form,
    ) -> Result<Value, LlmError> {
        self.check_key()?;

        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        let request_id = request_id_header(&resp);
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(status_error(status.as_u16(), request_id, &text));
        }

        serde_json::from_str(&text)
            .map_err(|e| LlmError::Decode(format!("OpenAI 응답 JSON 파싱 실패: {e}")))
    }
}

// ─── 중립 타입 → OpenAI wire format ───

fn content_parts_to_openai(parts: &[ContentPart]) -> Value {
    // 텍스트 하나뿐이면 문자열로 (OpenAI가 둘 다 받지만 기존 요청 형태를 유지).
    if let [ContentPart::Text(t)] = parts {
        return json!(t);
    }
    let blocks: Vec<Value> = parts
        .iter()
        .map(|p| match p {
            ContentPart::Text(t) => json!({ "type": "text", "text": t }),
            ContentPart::ImageDataUrl { url, detail } => json!({
                "type": "image_url",
                "image_url": { "url": url, "detail": detail.as_str() }
            }),
        })
        .collect();
    json!(blocks)
}

fn messages_to_openai(system: Option<&str>, messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);

    if let Some(sys) = system {
        out.push(json!({ "role": "system", "content": sys }));
    }

    for msg in messages {
        match msg {
            Message::User(parts) => out.push(json!({
                "role": "user",
                "content": content_parts_to_openai(parts),
            })),
            Message::Assistant { text, tool_calls } => {
                let mut m = json!({ "role": "assistant" });
                m["content"] = match text {
                    Some(t) => json!(t),
                    None => Value::Null,
                };
                if !tool_calls.is_empty() {
                    m["tool_calls"] = json!(
                        tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    // OpenAI는 arguments를 문자열로 받는다.
                                    "arguments": tc.arguments.to_string(),
                                }
                            }))
                            .collect::<Vec<_>>()
                    );
                }
                out.push(m);
            }
            Message::ToolResult { id, content } => out.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": content,
            })),
        }
    }

    out
}

fn tools_to_openai(tools: &[ToolDef]) -> Value {
    json!(
        tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            }))
            .collect::<Vec<_>>()
    )
}

/// 요청 바디 조립. 전송과 분리해 두어 wire format을 테스트로 고정할 수 있게 한다.
fn build_request_body(model: &str, req: &ChatRequest) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages_to_openai(req.system.as_deref(), &req.messages),
    });

    if let Some(t) = req.temperature {
        // f32를 그대로 직렬화하면 0.7이 0.699999988079071로 나간다. 샘플링 결과는 같지만
        // 요청 바디가 리팩터 이전과 달라 보이므로 소수 둘째 자리로 정리한다.
        body["temperature"] = json!((t as f64 * 100.0).round() / 100.0);
    }
    if let Some(m) = req.max_tokens {
        body["max_tokens"] = json!(m);
    }
    if req.response_format == ResponseFormat::Json {
        body["response_format"] = json!({ "type": "json_object" });
    }
    if !req.tools.is_empty() {
        body["tools"] = tools_to_openai(&req.tools);
    }

    body
}

fn parse_usage(v: &Value) -> Usage {
    Usage {
        input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
    }
}

#[async_trait]
impl ChatProvider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn chat(&self, model: &str, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let body = build_request_body(model, req);
        let resp = self.post(CHAT_URL, &body).await?;

        let choice = &resp["choices"][0];
        let msg = &choice["message"];

        let text = msg["content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from);

        let tool_calls: Vec<ToolCall> = msg["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|tc| ToolCall {
                        id: tc["id"].as_str().unwrap_or("").to_string(),
                        name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                        // arguments는 문자열로 오므로 객체로 되돌린다.
                        arguments: tc["function"]["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or_else(|| json!({})),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let stop = match choice["finish_reason"].as_str().unwrap_or("") {
            "tool_calls" => StopReason::ToolUse,
            "stop" => StopReason::EndTurn,
            "length" => StopReason::MaxTokens,
            _ => StopReason::Other,
        };

        if text.is_none() && tool_calls.is_empty() {
            return Err(LlmError::Decode(format!(
                "OpenAI 응답에 본문도 도구 호출도 없음: {}",
                truncate(&resp.to_string(), 300)
            )));
        }

        Ok(ChatResponse {
            text,
            tool_calls,
            stop,
            usage: parse_usage(&resp),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn embed(&self, model: &str, inputs: &[String]) -> Result<EmbeddingResult, LlmError> {
        let body = json!({ "model": model, "input": inputs });
        let resp = self.post(EMBEDDINGS_URL, &body).await?;

        let data = resp["data"]
            .as_array()
            .ok_or_else(|| LlmError::Decode("임베딩 응답에 data 배열이 없음".into()))?;

        // 응답 순서가 입력 순서와 같다는 보장이 없으므로 index로 정렬한다.
        let mut indexed: Vec<(usize, Vec<f32>)> = data
            .iter()
            .map(|item| {
                let idx = item["index"].as_u64().unwrap_or(0) as usize;
                let vec: Vec<f32> = item["embedding"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect()
                    })
                    .unwrap_or_default();
                (idx, vec)
            })
            .collect();
        indexed.sort_by_key(|(i, _)| *i);

        Ok(EmbeddingResult {
            vectors: indexed.into_iter().map(|(_, v)| v).collect(),
            usage: Usage {
                input_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                output_tokens: 0,
            },
        })
    }
}

#[async_trait]
impl ImageProvider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn generate(&self, model: &str, req: &ImageRequest) -> Result<ImageResult, LlmError> {
        let body = image_generation_body(model, req);
        let resp = self.post(IMAGES_URL, &body).await?;
        image_result(&resp)
    }

    async fn edit(&self, model: &str, req: &ImageEditRequest) -> Result<ImageResult, LlmError> {
        let image = reqwest::multipart::Part::bytes(req.base_png.clone())
            .file_name("base.png")
            .mime_str("image/png")
            .map_err(|e| LlmError::Decode(format!("편집 이미지 준비 실패: {e}")))?;
        let form = reqwest::multipart::Form::new()
            .text("model", model.to_string())
            .text("prompt", req.prompt.clone())
            .text("n", "1")
            .text("size", req.size.clone())
            .text("quality", req.quality.clone())
            .part("image", image);

        let resp = self.post_multipart(IMAGE_EDITS_URL, form).await?;
        image_result(&resp)
    }
}

/// 이미지 생성 요청 본문. `moderation` 을 지정하지 않으면 API 기본값(auto)이다. 공식 문서는
/// GPT Image 생성에 auto·low 를 두고 편집(edits)에 대해서는 말하지 않아, 편집 요청에는 싣지 않는다.
fn image_generation_body(model: &str, req: &ImageRequest) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": req.prompt,
        "n": 1,
        "size": req.size,
        "quality": req.quality,
    });
    if let Some(m) = req.moderation {
        body["moderation"] = json!(m.as_str());
    }
    body
}

/// 생성과 편집은 응답 형식이 같다.
fn image_result(resp: &Value) -> Result<ImageResult, LlmError> {
    let b64 = resp["data"][0]["b64_json"]
        .as_str()
        .ok_or_else(|| LlmError::Decode("이미지 응답에 b64_json이 없음".into()))?;

    Ok(ImageResult {
        b64_png: b64.to_string(),
        usage: Usage {
            input_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        },
    })
}

/// 응답 헤더의 요청 ID. 지원 문의에 쓴다.
fn request_id_header(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// non-2xx 응답을 오류로 바꾼다. 원문을 500자로 자르기 전에 `error.code`·`error.type`·요청 ID·
/// `moderation_details` 를 꺼내 둔다 — 예전에는 잘린 원문만 남아 차단 단계와 요청 ID 를 잃었다.
/// 요약(`body`)은 `error.message` 가 있으면 그것을, 없으면 원문을 자른 것이다. 오류 응답에는
/// 요청 본문이 들어 있지 않으므로 프롬프트가 로그에 남지 않는다.
fn status_error(status: u16, header_request_id: Option<String>, text: &str) -> LlmError {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    let err = parsed.as_ref().map(|v| &v["error"]);
    let str_of = |key: &str| {
        err.and_then(|e| e[key].as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let message = str_of("message");
    let request_id = header_request_id.or_else(|| {
        message.as_deref().and_then(|m| {
            m.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .find(|w| w.starts_with("req_") && w.len() > 4)
                .map(str::to_string)
        })
    });
    let moderation = err
        .map(|e| &e["moderation_details"])
        .filter(|m| m.is_object())
        .map(|m| ModerationDetails {
            stage: m["moderation_stage"].as_str().map(str::to_string),
            categories: m["categories"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        });
    LlmError::Status {
        provider: ProviderId::OpenAi,
        status,
        body: truncate(message.as_deref().unwrap_or(text), 500),
        detail: Box::new(ApiErrorDetail {
            code: str_of("code"),
            kind: str_of("type"),
            request_id,
            moderation,
        }),
    }
}

#[cfg(test)]
pub(crate) fn status_error_for_tests(status: u16, body: &str) -> LlmError {
    status_error(status, None, body)
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::ImageDetail;
    use super::*;

    #[test]
    fn single_text_part_serializes_as_string() {
        let v = content_parts_to_openai(&[ContentPart::Text("hi".into())]);
        assert_eq!(v, json!("hi"));
    }

    #[test]
    fn image_part_becomes_block_array() {
        let v = content_parts_to_openai(&[
            ContentPart::Text("look".into()),
            ContentPart::ImageDataUrl {
                url: "data:image/png;base64,AAA".into(),
                detail: ImageDetail::High,
            },
        ]);
        assert_eq!(v[0]["type"], "text");
        assert_eq!(v[1]["type"], "image_url");
        assert_eq!(v[1]["image_url"]["url"], "data:image/png;base64,AAA");
    }

    /// detail 은 호출부가 정한 값이 그대로 나가야 한다. 여기가 틀리면 저해상도
    /// 요청이 조용히 고해상도로 청구된다 — 응답은 멀쩡해서 눈치채기 어렵다.
    #[test]
    fn image_detail_is_sent_as_requested() {
        for (detail, expected) in [
            (ImageDetail::Low, "low"),
            (ImageDetail::High, "high"),
            (ImageDetail::Auto, "auto"),
        ] {
            let v = content_parts_to_openai(&[
                ContentPart::Text("q".into()),
                ContentPart::ImageDataUrl {
                    url: "data:image/png;base64,AAA".into(),
                    detail,
                },
            ]);
            assert_eq!(v[1]["image_url"]["detail"], expected, "{detail:?}");
        }
    }

    /// `user_image` 는 High 를 유지한다. 기본값이 바뀌면 옷 분석 3개 호출부의
    /// 정확도가 코드 변경 없이 떨어진다.
    #[test]
    fn user_image_defaults_to_high_detail() {
        let Message::User(parts) = Message::user_image("q", "data:image/png;base64,AAA") else {
            panic!("user_image 는 User 메시지를 만든다");
        };
        let v = content_parts_to_openai(&parts);
        assert_eq!(v[1]["image_url"]["detail"], "high");
    }

    #[test]
    fn tool_call_arguments_serialize_as_string() {
        let msgs = messages_to_openai(
            None,
            &[Message::Assistant {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "search".into(),
                    arguments: json!({"query": "olive"}),
                }],
            }],
        );
        let args = msgs[0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must be a string for OpenAI");
        assert_eq!(
            serde_json::from_str::<Value>(args).unwrap()["query"],
            "olive"
        );
    }

    #[test]
    fn system_prompt_is_prepended_as_message() {
        let msgs = messages_to_openai(Some("be brief"), &[Message::user_text("hi")]);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn tool_result_maps_to_tool_role() {
        let msgs = messages_to_openai(
            None,
            &[Message::ToolResult {
                id: "call_1".into(),
                content: "{}".into(),
            }],
        );
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "call_1");
    }

    // ─── 요청 바디 (wire format 고정) ───
    //
    // Anthropic 쪽 동명 테스트와 짝을 이룬다. 두 provider가 같은 중립 요청을
    // 각자의 형식으로 어떻게 다르게 직렬화하는지가 이 레이어의 존재 이유다.

    fn body_of(req: &ChatRequest) -> Value {
        build_request_body("gpt-4o-mini", req)
    }

    #[test]
    fn system_prompt_becomes_the_first_message() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]).system("be brief"));
        assert!(
            body.get("system").is_none(),
            "최상위 system 필드는 Anthropic 전용이다"
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "be brief");
    }

    #[test]
    fn temperature_is_sent_and_not_mangled_by_f32_serialization() {
        // 리팩터 이전 하드코딩되어 있던 값들.
        for (t, expected) in [
            (0.2f32, 0.2f64),
            (0.3, 0.3),
            (0.4, 0.4),
            (0.5, 0.5),
            (0.7, 0.7),
        ] {
            let mut req = ChatRequest::new(vec![Message::user_text("hi")]);
            req.temperature = Some(t);
            assert_eq!(
                body_of(&req)["temperature"],
                expected,
                "f32 직렬화 잔여값({})이 그대로 나가면 안 된다",
                t as f64
            );
        }
    }

    #[test]
    fn max_tokens_is_sent_verbatim_without_headroom() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]).max_tokens(500));
        assert_eq!(body["max_tokens"], 500);
    }

    #[test]
    fn json_mode_uses_native_response_format_and_leaves_system_alone() {
        let body = body_of(
            &ChatRequest::new(vec![Message::user_text("hi")])
                .system("역할 설명")
                .json(),
        );
        assert_eq!(body["response_format"]["type"], "json_object");
        // Anthropic과 달리 프롬프트에 JSON 지시를 덧붙이지 않는다.
        assert_eq!(body["messages"][0]["content"], "역할 설명");
    }

    #[test]
    fn tools_are_wrapped_in_a_function_object() {
        let req = ChatRequest::new(vec![Message::user_text("hi")]).tools(vec![ToolDef::new(
            "search",
            "검색",
            json!({"type": "object", "properties": {}}),
        )]);
        let tool = &body_of(&req)["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "search");
        assert_eq!(tool["function"]["parameters"]["type"], "object");
        assert!(
            tool.get("input_schema").is_none(),
            "Anthropic 형식이 새어나오면 안 된다"
        );
    }

    #[test]
    fn no_tools_field_when_none_declared() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]));
        assert!(body.get("tools").is_none());
    }

    /// 2026-10-18 실제 차단 응답 원문(요청 ID 만 그대로 둔 것). 예전에는 500자로 자른 원문만
    /// 남았다. 이제 코드·종류·요청 ID·차단 단계·분류를 따로 꺼낸다.
    const BLOCKED: &str = r#"{
  "error": {
    "message": "Your request was rejected by the safety system. If you believe this is an error, contact us at help.openai.com and include the request ID req_eb5d36417eb04e0d883ada7ffc4f97af. safety_violations=[sexual].",
    "type": "image_generation_user_error",
    "param": null,
    "code": "moderation_blocked",
    "moderation_details": {
      "moderation_stage": "output",
      "categories": ["sexual"]
    }
  }
}"#;

    #[test]
    fn a_moderation_block_keeps_its_code_request_id_and_stage() {
        let e = status_error(400, None, BLOCKED);
        assert!(e.is_moderation_blocked());
        assert!(!e.is_retryable());
        let d = e.api_detail().unwrap();
        assert_eq!(d.code.as_deref(), Some("moderation_blocked"));
        assert_eq!(d.kind.as_deref(), Some("image_generation_user_error"));
        assert_eq!(
            d.request_id.as_deref(),
            Some("req_eb5d36417eb04e0d883ada7ffc4f97af")
        );
        let m = d.moderation.as_ref().unwrap();
        assert_eq!(m.stage.as_deref(), Some("output"));
        assert_eq!(m.categories, vec!["sexual".to_string()]);
        let line = e.to_string();
        assert!(line.contains("code=moderation_blocked"));
        assert!(line.contains("request_id=req_eb5d36417eb04e0d883ada7ffc4f97af"));
        assert!(line.contains("moderation_stage=output categories=[sexual]"));
        // 요약은 error.message 다 — JSON 원문 전체가 아니다.
        assert!(!line.contains("\"param\""));
    }

    /// 단계 정보는 선택 항목이다. 없으면 단계를 모르는 것이고, 헤더의 요청 ID 가 메시지보다
    /// 먼저다. JSON 이 아닌 본문도 잘린 원문으로 남는다.
    #[test]
    fn optional_fields_stay_optional() {
        let no_details = r#"{"error":{"message":"blocked","type":"image_generation_user_error","code":"moderation_blocked"}}"#;
        let e = status_error(400, Some("req_header".into()), no_details);
        assert!(e.is_moderation_blocked());
        let d = e.api_detail().unwrap();
        assert_eq!(d.request_id.as_deref(), Some("req_header"));
        assert_eq!(d.moderation, None);
        assert!(!e.to_string().contains("moderation_stage"));

        let other = status_error(
            429,
            None,
            r#"{"error":{"message":"slow down","code":"rate_limit_exceeded"}}"#,
        );
        assert!(!other.is_moderation_blocked());
        assert!(other.is_retryable());

        let html = "<html>bad gateway</html>".repeat(40);
        let e = status_error(502, None, &html);
        assert_eq!(e.api_detail().unwrap(), &ApiErrorDetail::default());
        assert!(e.to_string().contains("bad gateway"));
        assert!(e.to_string().chars().count() < 600);
    }

    /// moderation 은 지정했을 때만 본문에 실린다(미지정 = API 기본값 auto).
    #[test]
    fn image_generation_carries_moderation_only_when_set() {
        let mut req = ImageRequest {
            prompt: "p".into(),
            size: "1024x1536".into(),
            quality: "low".into(),
            moderation: None,
        };
        assert!(image_generation_body("m", &req).get("moderation").is_none());
        req.moderation = Some(super::super::types::ImageModeration::Low);
        assert_eq!(image_generation_body("m", &req)["moderation"], "low");
        req.moderation = Some(super::super::types::ImageModeration::Auto);
        assert_eq!(image_generation_body("m", &req)["moderation"], "auto");
        use super::super::types::ImageModeration as M;
        assert_eq!(M::parse(" LOW "), Some(M::Low));
        assert_eq!(M::parse("auto"), Some(M::Auto));
        assert_eq!(M::parse("off"), None);
    }
}
