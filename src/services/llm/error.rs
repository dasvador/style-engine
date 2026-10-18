//! provider 공통 에러 타입.
//!
//! 재시도 가능 여부를 타입 안에서 판단할 수 있어야 상위 레이어가
//! provider별 상태코드 규칙을 알 필요가 없다.

use thiserror::Error;

use super::provider::ProviderId;

#[derive(Debug, Error)]
pub enum LlmError {
    /// 해당 provider가 이 기능을 제공하지 않음 (예: Anthropic 임베딩).
    #[error("{provider}는 {capability}를 지원하지 않습니다")]
    Unsupported {
        provider: ProviderId,
        capability: &'static str,
    },

    /// API 키 미설정 등 설정 오류.
    #[error("LLM 설정 오류: {0}")]
    Config(String),

    /// 네트워크/전송 실패.
    #[error("전송 실패: {0}")]
    Transport(String),

    /// 설정된 타임아웃 초과.
    #[error("타임아웃 ({0}초)")]
    Timeout(u64),

    /// non-2xx 응답. `body` 는 사람이 읽을 요약(잘린 원문)이고, 구조화할 수 있는 값은
    /// `detail` 에 따로 둔다 — 요약을 자르면서 request ID·차단 단계가 사라지지 않게.
    #[error("{provider} API 오류 ({status}){detail}: {body}")]
    Status {
        provider: ProviderId,
        status: u16,
        body: String,
        /// 박스로 둔다 — 모든 `Result<_, LlmError>` 의 크기를 키우지 않게.
        detail: Box<ApiErrorDetail>,
    },

    /// 응답 형식이 예상과 다름 (JSON 파싱 실패, 필드 누락 등).
    #[error("응답 해석 실패: {0}")]
    Decode(String),

    /// Anthropic 안전 분류기 거절. HTTP 200으로 오기 때문에 상태코드로는 안 잡힌다.
    #[error("모델이 요청을 거절했습니다 (category: {category})")]
    Refusal { category: String },
}

/// 오류 응답에서 꺼낸 값. 전부 선택 항목이다 — provider 가 주지 않으면 `None` 이다.
/// 요청 본문(프롬프트)이나 API 키는 담지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiErrorDetail {
    /// `error.code` (예: `moderation_blocked`).
    pub code: Option<String>,
    /// `error.type` (예: `image_generation_user_error`).
    pub kind: Option<String>,
    /// 지원 문의에 쓰는 요청 ID. 응답 헤더 `x-request-id`, 없으면 메시지 안의 `req_…`.
    pub request_id: Option<String>,
    /// `error.moderation_details` — `moderation_blocked` 일 때만, 그것도 선택적으로 온다.
    pub moderation: Option<ModerationDetails>,
}

/// 안전 검사 차단의 대략적인 정보(공식 문서: 내부 분류기 점수가 아닌 디버깅용 맥락).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModerationDetails {
    /// `input`·`output`·`unknown`. 값이 없으면 `None` — 단계를 모르는 것이다.
    pub stage: Option<String>,
    /// `sexual`·`violence` 같은 대략적인 분류.
    pub categories: Vec<String>,
}

impl std::fmt::Display for ApiErrorDetail {
    /// 로그 한 줄에 붙는 꼬리표. 값이 없으면 아무것도 쓰지 않는다.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(code) = &self.code {
            write!(f, " code={code}")?;
        }
        if let Some(id) = &self.request_id {
            write!(f, " request_id={id}")?;
        }
        if let Some(m) = &self.moderation {
            write!(
                f,
                " moderation_stage={} categories=[{}]",
                m.stage.as_deref().unwrap_or("unknown"),
                m.categories.join(",")
            )?;
        }
        Ok(())
    }
}

impl LlmError {
    /// 안전 검사에 막힌 요청인가(`error.code == "moderation_blocked"`). 같은 요청을 다시 보내도
    /// 결과가 같을 가능성이 높아 재시도하지 않는다.
    pub fn is_moderation_blocked(&self) -> bool {
        matches!(self, LlmError::Status { detail, .. } if detail.code.as_deref() == Some("moderation_blocked"))
    }

    /// 구조화된 오류 정보. `Status` 가 아니면 `None`.
    pub fn api_detail(&self) -> Option<&ApiErrorDetail> {
        match self {
            LlmError::Status { detail, .. } => Some(detail.as_ref()),
            _ => None,
        }
    }

    /// 같은 요청을 그대로 다시 보내볼 가치가 있는가.
    ///
    /// 4xx는 요청 자체가 잘못된 것이므로 재시도해도 같은 결과다. 단 429(rate limit)와
    /// 408(request timeout)은 예외.
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Transport(_) | LlmError::Timeout(_) => true,
            LlmError::Status { status, .. } => *status == 429 || *status == 408 || *status >= 500,
            // Decode 실패는 모델 출력이 흔들린 경우라 재시도로 회복되는 일이 잦다.
            LlmError::Decode(_) => true,
            LlmError::Unsupported { .. } | LlmError::Config(_) | LlmError::Refusal { .. } => false,
        }
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        LlmError::Transport(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_are_not_retried() {
        let e = LlmError::Status {
            provider: ProviderId::OpenAi,
            status: 400,
            body: "bad request".into(),
            detail: Default::default(),
        };
        assert!(!e.is_retryable());
    }

    #[test]
    fn rate_limit_and_server_errors_are_retried() {
        for status in [408, 429, 500, 503] {
            let e = LlmError::Status {
                provider: ProviderId::OpenAi,
                status,
                body: String::new(),
                detail: Default::default(),
            };
            assert!(e.is_retryable(), "status {status} should be retryable");
        }
    }

    #[test]
    fn refusal_is_terminal() {
        let e = LlmError::Refusal {
            category: "cyber".into(),
        };
        assert!(!e.is_retryable());
    }
}
