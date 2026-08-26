//! provider 인터페이스.
//!
//! 능력(capability)별로 trait을 나눈 이유: provider마다 제공 범위가 다르다.
//! Anthropic은 chat/vision은 되지만 임베딩·이미지 생성 엔드포인트가 없다.
//! 하나의 거대한 trait으로 묶으면 모든 구현체가 `unimplemented!()`을 들고 있게 되고,
//! "이 provider로 교체 가능한가"를 컴파일 타임에 판단할 수 없게 된다.

use async_trait::async_trait;

use super::error::LlmError;
use super::types::{ChatRequest, ChatResponse, EmbeddingResult, ImageRequest, ImageResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    OpenAi,
    Anthropic,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::OpenAi => "openai",
            ProviderId::Anthropic => "anthropic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "openai" => Some(ProviderId::OpenAi),
            "anthropic" | "claude" => Some(ProviderId::Anthropic),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 텍스트/비전 대화. 도구 호출 포함.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// `model`은 task 설정에서 온다 — 구현체가 모델명을 하드코딩해서는 안 된다.
    async fn chat(&self, model: &str, req: &ChatRequest) -> Result<ChatResponse, LlmError>;
}

/// 텍스트 임베딩.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    async fn embed(&self, model: &str, inputs: &[String]) -> Result<EmbeddingResult, LlmError>;
}

/// 이미지 생성.
#[async_trait]
pub trait ImageProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    async fn generate(&self, model: &str, req: &ImageRequest) -> Result<ImageResult, LlmError>;
}
