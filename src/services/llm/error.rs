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

    /// non-2xx 응답.
    #[error("{provider} API 오류 ({status}): {body}")]
    Status {
        provider: ProviderId,
        status: u16,
        body: String,
    },

    /// 응답 형식이 예상과 다름 (JSON 파싱 실패, 필드 누락 등).
    #[error("응답 해석 실패: {0}")]
    Decode(String),

    /// Anthropic 안전 분류기 거절. HTTP 200으로 오기 때문에 상태코드로는 안 잡힌다.
    #[error("모델이 요청을 거절했습니다 (category: {category})")]
    Refusal { category: String },
}

impl LlmError {
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
