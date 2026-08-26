//! Provider-neutral request/response types.
//!
//! 여기 있는 타입은 어떤 provider의 wire format도 흉내내지 않는다.
//! OpenAI의 `tool_calls`나 Anthropic의 `tool_use` 같은 표현은 각 provider 구현체가
//! 이 타입들로부터 직렬화/역직렬화한다. 호출부는 wire format을 몰라야 한다.

use serde_json::Value;

use super::usage::Usage;

// ─── 입력 ───

/// user 메시지를 구성하는 조각. 텍스트와 이미지를 섞을 수 있다.
#[derive(Debug, Clone)]
pub enum ContentPart {
    Text(String),
    /// `data:image/png;base64,...` 형태의 data URL.
    /// provider 마다 요구 형식이 달라서 (OpenAI는 data URL 그대로, Anthropic은
    /// media_type/data 분해) 여기서는 원본 그대로 들고 있다가 각자 변환한다.
    ImageDataUrl(String),
}

/// 모델이 요청한 도구 호출 1건.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// 파싱된 인자. provider가 문자열로 주든 객체로 주든 여기서는 항상 객체.
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum Message {
    User(Vec<ContentPart>),
    Assistant {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    /// 도구 실행 결과. `id`는 대응하는 `ToolCall::id`.
    ToolResult {
        id: String,
        content: String,
    },
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message::User(vec![ContentPart::Text(text.into())])
    }

    pub fn user_image(text: impl Into<String>, image_data_url: impl Into<String>) -> Self {
        Message::User(vec![
            ContentPart::Text(text.into()),
            ContentPart::ImageDataUrl(image_data_url.into()),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema. OpenAI는 `function.parameters`, Anthropic은 `input_schema`로 나간다.
    pub input_schema: Value,
}

impl ToolDef {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    Text,
    /// JSON 객체 하나만 응답. provider별 구현 차이는 아래 주석 참고.
    ///
    /// - OpenAI: `response_format: {"type": "json_object"}` (네이티브 지원)
    /// - Anthropic: 스키마 없는 JSON 모드가 없어서 시스템 프롬프트 지시 + 코드펜스 제거로 구현.
    ///   두 경우 모두 호출부는 `ChatResponse::text`에서 바로 파싱 가능한 JSON을 받는다.
    Json,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub response_format: ResponseFormat,
    /// `None`이면 task 설정값을 사용한다.
    pub temperature: Option<f32>,
    /// `None`이면 task 설정값을 사용한다.
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            system: None,
            messages,
            tools: Vec::new(),
            response_format: ResponseFormat::Text,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn json(mut self) -> Self {
        self.response_format = ResponseFormat::Json;
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDef>) -> Self {
        self.tools = tools;
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

// ─── 출력 ───

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    /// Anthropic 안전 분류기가 요청을 거절한 경우. OpenAI에는 대응 개념이 없다.
    Refusal,
    Other,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop: StopReason,
    pub usage: Usage,
}

impl ChatResponse {
    /// 텍스트 본문. 없으면 빈 문자열.
    pub fn text_or_empty(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    /// JSON 응답을 타입으로 역직렬화.
    pub fn parse_json<T: serde::de::DeserializeOwned>(&self) -> Result<T, super::LlmError> {
        let text = self
            .text
            .as_deref()
            .ok_or_else(|| super::LlmError::Decode("응답에 텍스트 본문이 없음".into()))?;
        serde_json::from_str(text)
            .map_err(|e| super::LlmError::Decode(format!("JSON 파싱 실패: {e} — 본문: {text}")))
    }
}

// ─── 임베딩 ───

#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    /// 입력과 같은 순서의 벡터들.
    pub vectors: Vec<Vec<f32>>,
    pub usage: Usage,
}

// ─── 이미지 생성 ───

#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub prompt: String,
    pub size: String,
    pub quality: String,
}

#[derive(Debug, Clone)]
pub struct ImageResult {
    /// base64 인코딩된 PNG 바이트.
    pub b64_png: String,
    pub usage: Usage,
}
