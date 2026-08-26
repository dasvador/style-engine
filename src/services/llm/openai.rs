//! OpenAI provider — Chat Completions / Embeddings / Images.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::error::LlmError;
use super::provider::{ChatProvider, EmbeddingProvider, ImageProvider, ProviderId};
use super::types::{
    ChatRequest, ChatResponse, ContentPart, EmbeddingResult, ImageRequest, ImageResult, Message,
    ResponseFormat, StopReason, ToolCall, ToolDef,
};
use super::usage::Usage;

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";
const IMAGES_URL: &str = "https://api.openai.com/v1/images/generations";

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
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(LlmError::Status {
                provider: ProviderId::OpenAi,
                status: status.as_u16(),
                body: truncate(&text, 500),
            });
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
            ContentPart::ImageDataUrl(url) => json!({
                "type": "image_url",
                "image_url": { "url": url, "detail": "high" }
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
        let body = json!({
            "model": model,
            "prompt": req.prompt,
            "n": 1,
            "size": req.size,
            "quality": req.quality,
        });

        let resp = self.post(IMAGES_URL, &body).await?;

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
    fn single_text_part_serializes_as_string() {
        let v = content_parts_to_openai(&[ContentPart::Text("hi".into())]);
        assert_eq!(v, json!("hi"));
    }

    #[test]
    fn image_part_becomes_block_array() {
        let v = content_parts_to_openai(&[
            ContentPart::Text("look".into()),
            ContentPart::ImageDataUrl("data:image/png;base64,AAA".into()),
        ]);
        assert_eq!(v[0]["type"], "text");
        assert_eq!(v[1]["type"], "image_url");
        assert_eq!(v[1]["image_url"]["url"], "data:image/png;base64,AAA");
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
}
