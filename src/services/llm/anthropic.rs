//! Anthropic provider — Messages API.
//!
//! Rust용 공식 SDK가 없어 raw HTTP로 호출한다.
//!
//! OpenAI와 다른 점 중 이 레이어가 흡수하는 것들:
//!
//! - **system 프롬프트가 메시지가 아니라 최상위 필드다.**
//! - **`temperature`를 보내면 안 된다.** 현재 세대 모델(Opus 5, Sonnet 5, Opus 4.7/4.8,
//!   Fable 5)은 샘플링 파라미터를 제거했고, 보내면 400으로 거절한다. task 설정의
//!   temperature는 여기서 의도적으로 버린다.
//! - **스키마 없는 JSON 모드가 없다.** OpenAI의 `response_format: json_object`에 대응하는
//!   기능이 없어서, 시스템 프롬프트 지시 + 코드펜스 제거로 같은 계약을 만든다.
//! - **thinking이 기본으로 켜져 있고 그 토큰도 `max_tokens`에서 나간다.** 호출부가 요청한
//!   `max_tokens`는 "답변 길이"를 뜻하므로, 사고 토큰 여유분을 여기서 더해준다.
//! - **거절(refusal)이 HTTP 200으로 온다.** 상태코드로는 안 잡히므로 `stop_reason`을 본다.
//! - **도구 결과는 user 메시지 하나에 모아 보내야 한다.** 나눠 보내면 모델이 병렬 도구
//!   호출을 점점 안 하게 된다.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::error::LlmError;
use super::provider::{ChatProvider, ProviderId};
use super::types::{
    ChatRequest, ChatResponse, ContentPart, Message, ResponseFormat, StopReason, ToolCall, ToolDef,
};
use super::usage::Usage;

const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// 사고 토큰이 `max_tokens`를 잠식하기 때문에 얹어주는 여유분.
/// 이게 없으면 짧은 추출 task(max_tokens=500)가 답변을 쓰기도 전에 잘린다.
const THINKING_HEADROOM_TOKENS: u32 = 4096;

const JSON_INSTRUCTION: &str = "\n\n반드시 유효한 JSON 객체 하나만 출력하세요. \
설명 문장이나 마크다운 코드펜스를 덧붙이지 마세요.";

pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
    effort: String,
}

impl AnthropicProvider {
    pub fn new(http: reqwest::Client, api_key: String, effort: String) -> Self {
        Self {
            http,
            api_key,
            effort,
        }
    }
}

// ─── 중립 타입 → Anthropic wire format ───

/// `data:image/png;base64,AAA` → `("image/png", "AAA")`
fn split_data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.strip_suffix(";base64")?;
    Some((media_type, data))
}

fn content_parts_to_anthropic(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|p| match p {
            ContentPart::Text(t) => json!({ "type": "text", "text": t }),
            ContentPart::ImageDataUrl(url) => match split_data_url(url) {
                Some((media_type, data)) => json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media_type, "data": data }
                }),
                // data URL이 아니면 원격 URL로 간주한다.
                None => json!({
                    "type": "image",
                    "source": { "type": "url", "url": url }
                }),
            },
        })
        .collect()
}

fn messages_to_anthropic(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(messages.len());
    // 연속된 도구 결과를 하나의 user 메시지로 모으기 위한 버퍼.
    let mut pending_tool_results: Vec<Value> = Vec::new();

    let flush = |buf: &mut Vec<Value>, out: &mut Vec<Value>| {
        if !buf.is_empty() {
            out.push(json!({ "role": "user", "content": std::mem::take(buf) }));
        }
    };

    for msg in messages {
        match msg {
            Message::ToolResult { id, content } => {
                pending_tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": content,
                }));
            }
            Message::User(parts) => {
                flush(&mut pending_tool_results, &mut out);
                out.push(json!({
                    "role": "user",
                    "content": content_parts_to_anthropic(parts),
                }));
            }
            Message::Assistant { text, tool_calls } => {
                flush(&mut pending_tool_results, &mut out);
                let mut blocks: Vec<Value> = Vec::new();
                if let Some(t) = text.as_deref().filter(|t| !t.trim().is_empty()) {
                    blocks.push(json!({ "type": "text", "text": t }));
                }
                for tc in tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                // 빈 assistant 메시지는 API가 거절한다.
                if !blocks.is_empty() {
                    out.push(json!({ "role": "assistant", "content": blocks }));
                }
            }
        }
    }

    flush(&mut pending_tool_results, &mut out);
    out
}

fn tools_to_anthropic(tools: &[ToolDef]) -> Value {
    json!(
        tools
            .iter()
            .map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            }))
            .collect::<Vec<_>>()
    )
}

/// 모델이 지시를 어기고 코드펜스로 감싸는 경우를 대비한 방어.
/// JSON 모드에서 호출부는 항상 바로 파싱 가능한 문자열을 받아야 한다.
fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    // ```json / ``` 뒤의 첫 줄바꿈까지가 여는 펜스.
    let body = match after_open.split_once('\n') {
        Some((_lang, rest)) => rest,
        None => return trimmed.to_string(),
    };
    body.trim_end()
        .strip_suffix("```")
        .unwrap_or(body)
        .trim()
        .to_string()
}

/// 요청 바디 조립. 전송과 분리해 두어 wire format을 테스트로 고정할 수 있게 한다.
fn build_request_body(model: &str, req: &ChatRequest, effort: &str) -> Value {
    let json_mode = req.response_format == ResponseFormat::Json;

    let system = match (&req.system, json_mode) {
        (Some(s), true) => Some(format!("{s}{JSON_INSTRUCTION}")),
        (Some(s), false) => Some(s.clone()),
        (None, true) => Some(JSON_INSTRUCTION.trim().to_string()),
        (None, false) => None,
    };

    let mut body = json!({
        "model": model,
        "messages": messages_to_anthropic(&req.messages),
        "max_tokens": req.max_tokens.unwrap_or(1024) + THINKING_HEADROOM_TOKENS,
        "output_config": { "effort": effort },
    });

    if let Some(sys) = system {
        body["system"] = json!(sys);
    }
    if !req.tools.is_empty() {
        body["tools"] = tools_to_anthropic(&req.tools);
    }
    // temperature는 의도적으로 넣지 않는다 — 모듈 상단 주석 참고.

    body
}

#[async_trait]
impl ChatProvider for AnthropicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    async fn chat(&self, model: &str, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError::Config(
                "ANTHROPIC_API_KEY가 설정되지 않았습니다".into(),
            ));
        }

        let json_mode = req.response_format == ResponseFormat::Json;
        let body = build_request_body(model, req, &self.effort);

        let resp = self
            .http
            .post(MESSAGES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(LlmError::Status {
                provider: ProviderId::Anthropic,
                status: status.as_u16(),
                body: truncate(&text, 500),
            });
        }

        let resp: Value = serde_json::from_str(&text)
            .map_err(|e| LlmError::Decode(format!("Anthropic 응답 JSON 파싱 실패: {e}")))?;

        // 거절은 200으로 온다. content를 읽기 전에 먼저 확인한다.
        if resp["stop_reason"].as_str() == Some("refusal") {
            return Err(LlmError::Refusal {
                category: resp["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }

        let blocks = resp["content"]
            .as_array()
            .ok_or_else(|| LlmError::Decode("Anthropic 응답에 content 배열이 없음".into()))?;

        let mut text_parts: Vec<&str> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(t) = block["text"].as_str() {
                        text_parts.push(t);
                    }
                }
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: block["id"].as_str().unwrap_or("").to_string(),
                    name: block["name"].as_str().unwrap_or("").to_string(),
                    arguments: block["input"].clone(),
                }),
                // thinking 블록 등은 이 레이어의 관심사가 아니다.
                _ => {}
            }
        }

        let joined = text_parts.join("");
        let text = if joined.trim().is_empty() {
            None
        } else if json_mode {
            Some(strip_code_fence(&joined))
        } else {
            Some(joined)
        };

        if text.is_none() && tool_calls.is_empty() {
            return Err(LlmError::Decode(
                "Anthropic 응답에 본문도 도구 호출도 없음".into(),
            ));
        }

        let stop = match resp["stop_reason"].as_str().unwrap_or("") {
            "tool_use" => StopReason::ToolUse,
            "end_turn" | "stop_sequence" => StopReason::EndTurn,
            "max_tokens" => StopReason::MaxTokens,
            "refusal" => StopReason::Refusal,
            _ => StopReason::Other,
        };

        Ok(ChatResponse {
            text,
            tool_calls,
            stop,
            usage: Usage {
                input_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
                output_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            },
        })
    }
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_data_url() {
        let (mt, data) = split_data_url("data:image/png;base64,AAAB").unwrap();
        assert_eq!(mt, "image/png");
        assert_eq!(data, "AAAB");
    }

    #[test]
    fn non_data_url_falls_back_to_url_source() {
        let blocks = content_parts_to_anthropic(&[ContentPart::ImageDataUrl(
            "https://example.com/a.png".into(),
        )]);
        assert_eq!(blocks[0]["source"]["type"], "url");
    }

    #[test]
    fn consecutive_tool_results_merge_into_one_user_message() {
        let msgs = messages_to_anthropic(&[
            Message::user_text("hi"),
            Message::Assistant {
                text: None,
                tool_calls: vec![
                    ToolCall {
                        id: "a".into(),
                        name: "t".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "b".into(),
                        name: "t".into(),
                        arguments: json!({}),
                    },
                ],
            },
            Message::ToolResult {
                id: "a".into(),
                content: "1".into(),
            },
            Message::ToolResult {
                id: "b".into(),
                content: "2".into(),
            },
        ]);

        assert_eq!(
            msgs.len(),
            3,
            "tool results must collapse into a single message"
        );
        assert_eq!(msgs[2]["role"], "user");
        let blocks = msgs[2]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["tool_use_id"], "a");
        assert_eq!(blocks[1]["tool_use_id"], "b");
    }

    #[test]
    fn empty_assistant_message_is_dropped() {
        let msgs = messages_to_anthropic(&[
            Message::user_text("hi"),
            Message::Assistant {
                text: Some("  ".into()),
                tool_calls: vec![],
            },
        ]);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn tool_call_input_stays_an_object() {
        let msgs = messages_to_anthropic(&[Message::Assistant {
            text: None,
            tool_calls: vec![ToolCall {
                id: "a".into(),
                name: "search".into(),
                arguments: json!({"query": "olive"}),
            }],
        }]);
        // OpenAI와 달리 문자열이 아니라 객체여야 한다.
        assert_eq!(msgs[0]["content"][0]["input"]["query"], "olive");
    }

    #[test]
    fn strips_json_code_fence() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("{\"a\":1}"), "{\"a\":1}");
    }

    // ─── 요청 바디 (wire format 고정) ───
    //
    // 실 API 호출 없이 검증 가능한 계약. Anthropic 키가 없는 환경에서도
    // OpenAI와의 차이가 실수로 무너지지 않게 잡아둔다.

    fn body_of(req: &ChatRequest) -> Value {
        build_request_body("claude-opus-5", req, "low")
    }

    #[test]
    fn system_prompt_is_a_top_level_field_not_a_message() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]).system("be brief"));
        assert_eq!(body["system"], "be brief");
        // messages에는 user만 있어야 한다.
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn temperature_is_never_sent() {
        let mut req = ChatRequest::new(vec![Message::user_text("hi")]);
        req.temperature = Some(0.7);
        let body = body_of(&req);
        assert!(
            body.get("temperature").is_none(),
            "현행 Anthropic 모델은 샘플링 파라미터를 400으로 거절한다"
        );
    }

    #[test]
    fn max_tokens_gets_thinking_headroom() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]).max_tokens(500));
        assert_eq!(body["max_tokens"], 500 + THINKING_HEADROOM_TOKENS);
    }

    #[test]
    fn json_mode_appends_instruction_to_system() {
        let body = body_of(
            &ChatRequest::new(vec![Message::user_text("hi")])
                .system("역할 설명")
                .json(),
        );
        let sys = body["system"].as_str().unwrap();
        assert!(sys.starts_with("역할 설명"));
        assert!(sys.contains("JSON"));
    }

    #[test]
    fn json_mode_without_system_still_carries_the_instruction() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]).json());
        assert!(body["system"].as_str().unwrap().contains("JSON"));
    }

    #[test]
    fn effort_is_sent_inside_output_config() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]));
        assert_eq!(body["output_config"]["effort"], "low");
    }

    #[test]
    fn tools_use_input_schema_not_function_wrapper() {
        let req = ChatRequest::new(vec![Message::user_text("hi")]).tools(vec![ToolDef::new(
            "search",
            "검색",
            json!({"type": "object", "properties": {}}),
        )]);
        let body = body_of(&req);
        let tool = &body["tools"][0];
        assert_eq!(tool["name"], "search");
        assert_eq!(tool["input_schema"]["type"], "object");
        assert!(
            tool.get("function").is_none(),
            "OpenAI 형식이 새어나오면 안 된다"
        );
        assert!(tool.get("type").is_none());
    }

    #[test]
    fn no_tools_field_when_none_declared() {
        let body = body_of(&ChatRequest::new(vec![Message::user_text("hi")]));
        assert!(body.get("tools").is_none());
    }
}
