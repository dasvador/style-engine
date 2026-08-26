//! 토큰 사용량 · 지연시간 · 추정 비용 계측.
//!
//! 모든 LLM 호출이 provider 레이어 한 곳을 지나가므로, 계측도 여기서 한 번만 한다.
//! 로그는 구조화된 tracing 필드로 남긴다 — `llm_call` 이벤트만 긁으면
//! task별 비용/지연 분포를 뽑을 수 있다.

use std::time::Duration;

use super::config::LlmTask;
use super::provider::ProviderId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// 백만 토큰당 단가 (USD). (모델 접두사, input, output).
///
/// 최장 접두사 우선으로 매칭한다 — "gpt-4o-mini"가 "gpt-4o"보다 먼저 잡혀야 한다.
/// 단가는 고정이 아니므로 여기 값은 "추정치"이고, 정확한 청구액은 provider 대시보드가 기준이다.
/// 이 표의 목적은 절대 금액이 아니라 task 간 상대 비용 비교와 회귀 감지다.
const PRICING_PER_MTOK: &[(&str, f64, f64)] = &[
    // OpenAI
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.00),
    ("text-embedding-3-small", 0.02, 0.0),
    ("text-embedding-3-large", 0.13, 0.0),
    // Anthropic
    ("claude-opus-5", 5.00, 25.00),
    ("claude-opus-4-8", 5.00, 25.00),
    ("claude-sonnet-5", 3.00, 15.00),
    ("claude-sonnet-4-6", 3.00, 15.00),
    ("claude-haiku-4-5", 1.00, 5.00),
];

/// 추정 비용(USD). 단가표에 없는 모델(이미지 생성 등 토큰 과금이 아닌 경우 포함)은 `None`.
/// 모르는 값을 0으로 채우면 "공짜로 돌고 있다"고 오독되므로 명시적으로 비운다.
pub fn estimate_cost_usd(model: &str, usage: &Usage) -> Option<f64> {
    let (_, input_rate, output_rate) = PRICING_PER_MTOK
        .iter()
        .filter(|(prefix, _, _)| model.starts_with(prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())?;

    Some(
        (usage.input_tokens as f64 / 1_000_000.0) * input_rate
            + (usage.output_tokens as f64 / 1_000_000.0) * output_rate,
    )
}

/// 호출 1건의 계측 결과를 구조화 로그로 남긴다.
pub fn record_call(
    task: LlmTask,
    provider: ProviderId,
    model: &str,
    usage: &Usage,
    elapsed: Duration,
    attempts: u32,
) {
    let cost = estimate_cost_usd(model, usage);
    tracing::info!(
        event = "llm_call",
        task = task.name(),
        provider = provider.as_str(),
        model = model,
        input_tokens = usage.input_tokens,
        output_tokens = usage.output_tokens,
        latency_ms = elapsed.as_millis() as u64,
        attempts = attempts,
        cost_usd = cost,
        "LLM call complete"
    );
}

/// 재시도까지 모두 실패한 호출.
pub fn record_failure(
    task: LlmTask,
    provider: ProviderId,
    model: &str,
    elapsed: Duration,
    attempts: u32,
    error: &super::LlmError,
) {
    tracing::warn!(
        event = "llm_call_failed",
        task = task.name(),
        provider = provider.as_str(),
        model = model,
        latency_ms = elapsed.as_millis() as u64,
        attempts = attempts,
        error = %error,
        "LLM call failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
        };
        // "gpt-4o-mini"가 "gpt-4o"보다 먼저 잡혀야 한다.
        assert_eq!(estimate_cost_usd("gpt-4o-mini", &usage), Some(0.15));
        assert_eq!(estimate_cost_usd("gpt-4o", &usage), Some(2.50));
    }

    #[test]
    fn unknown_model_has_no_estimate() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 100,
        };
        assert_eq!(estimate_cost_usd("gpt-image-2", &usage), None);
    }

    #[test]
    fn output_tokens_priced_separately() {
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 1_000_000,
        };
        assert_eq!(estimate_cost_usd("claude-opus-5", &usage), Some(25.0));
    }
}
