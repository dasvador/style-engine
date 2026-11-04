//! E2E 전용 고정 응답. `e2e-stub` 기능으로 빌드하고 `WARDROBE_E2E_STUB=1` 일 때만 켜진다.
//! 운영 빌드에는 없다. 실제 Rust 서버 + 일회용 DB + MCP 를 모델·날씨 API 비용 없이 잇기 위한 것이다.
//!
//! 후보 지시문의 슬롯별 목록에서 앞쪽 옷의 참조를 그대로 골라 코디 다섯 개를 만든다. 기온은
//! `WARDROBE_STUB_TEMPERATURE`(기본 15).

use async_trait::async_trait;
use serde_json::json;

use super::error::LlmError;
use super::provider::{ChatProvider, EmbeddingProvider, ProviderId};
use super::types::{ChatRequest, ChatResponse, ContentPart, EmbeddingResult, Message, StopReason};
use super::usage::Usage;
use crate::models::weather::CurrentWeather;

pub struct StubProvider;

pub fn weather() -> CurrentWeather {
    let temperature = std::env::var("WARDROBE_STUB_TEMPERATURE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15.0);
    CurrentWeather {
        temperature,
        apparent_temperature: temperature,
        humidity: 50.0,
        wind_speed: 1.0,
        weather_code: 0,
        weather_description: "맑음(스텁)".to_string(),
    }
}

/// `[상의 후보]` 뒤의 `- 이름 | id:T1 | ...` 줄에서 (참조, 이름)을 읽는다.
fn slot_items(prompt: &str, heading: &str) -> Vec<(String, String)> {
    let Some(start) = prompt.find(heading) else {
        return Vec::new();
    };
    prompt[start + heading.len()..]
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| l.starts_with("- "))
        .filter_map(|l| {
            let mut parts = l.trim_start_matches("- ").split(" | ");
            let name = parts.next()?.to_string();
            let id = parts.next()?.strip_prefix("id:")?.to_string();
            Some((id, name))
        })
        .collect()
}

#[async_trait]
impl ChatProvider for StubProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn chat(&self, _model: &str, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let prompt: String = req
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::User(parts) => Some(parts),
                _ => None,
            })
            .flatten()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let slots = [
            ("상의", slot_items(&prompt, "[상의 후보]")),
            ("하의", slot_items(&prompt, "[하의 후보]")),
            ("아우터", slot_items(&prompt, "[아우터 후보]")),
            ("신발", slot_items(&prompt, "[신발 후보]")),
            ("가방", slot_items(&prompt, "[가방 후보]")),
        ];
        let candidates: Vec<_> = (0..5)
            .map(|i| {
                let outfit: Vec<_> = slots
                    .iter()
                    .filter(|(_, items)| !items.is_empty())
                    .map(|(category, items)| {
                        let (id, name) = &items[i % items.len()];
                        json!({ "category": category, "id": id, "name": name, "reason": "스텁 이유" })
                    })
                    .collect();
                json!({
                    "recommendation": format!("스텁 후보 {}", i + 1),
                    "outfit": outfit,
                    "weather_summary": "맑음(스텁)",
                    "tips": [],
                })
            })
            .collect();
        Ok(ChatResponse {
            text: Some(json!({ "candidates": candidates }).to_string()),
            tool_calls: Vec::new(),
            stop: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for StubProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn embed(&self, _model: &str, inputs: &[String]) -> Result<EmbeddingResult, LlmError> {
        Ok(EmbeddingResult {
            vectors: inputs.iter().map(|_| vec![0.1_f32; 1536]).collect(),
            usage: Usage::default(),
        })
    }
}
