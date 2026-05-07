use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::db::clothing_repo;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::services::outfit_scorer;
use crate::services::weather as weather_service;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(chat))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    reply: String,
    items: Vec<ChatItem>,
}

#[derive(Debug, Serialize)]
struct ChatItem {
    slot: String,
    category: String,
    name: String,
    owned: bool,
}

async fn chat(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    let user_id = &auth.user_id;
    if state.openai_api_key.is_empty() || state.openai_api_key == "sk-your-key-here" {
        return Err(AppError::BadRequest(
            "OPENAI_API_KEY is not configured".to_string(),
        ));
    }

    let clothes = clothing_repo::list_clothing(&state.db).await?;

    // 날씨
    let mut temperature: Option<f64> = None;
    let weather_hint = match crate::db::region_repo::get_region(&state.db).await {
        Ok(Some(region)) => {
            match weather_service::fetch_weather(
                &state.http_client,
                &state.kma_api_key,
                region.latitude,
                region.longitude,
            )
            .await
            {
                Ok(w) => {
                    temperature = Some(w.temperature);
                    format!(
                        "현재 날씨: {}°C (체감 {}°C), {}, 습도 {}%",
                        w.temperature, w.apparent_temperature, w.weather_description, w.humidity
                    )
                }
                Err(_) => String::new(),
            }
        }
        _ => String::new(),
    };

    // anchor 탐색
    let anchors = find_anchors(&body.message, &clothes);

    // 유저 프로파일
    let user_profile = sqlx::query_as::<_, crate::models::user_profile::UserStyleProfile>(
        "SELECT * FROM user_style_profile WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    // 피드백 3층 로드
    let feedback_ctx = {
        let item_scores = crate::db::feedback_repo::get_item_adjustments(&state.db, user_id)
            .await.unwrap_or_default();
        let pref_scores = crate::db::feedback_repo::get_preference_scores(&state.db, user_id)
            .await.unwrap_or_default();
        outfit_scorer::FeedbackContext {
            item_adj: item_scores.into_iter().map(|s| (s.item_name, s.score_adjustment)).collect(),
            preference: pref_scores.into_iter().map(|s| (s.reason_tag, s.score)).collect(),
        }
    };

    // ─── 서버가 최종 조합 결정 (LLM 선택권 없음) ───
    let (fixed_outfit, outfit_items) = if !anchors.is_empty() {
        let result = build_final_outfit(anchors[0], &clothes, user_profile.as_ref(), temperature, &feedback_ctx);
        match result {
            Some((outfit_desc, items)) => (outfit_desc, items),
            None => (String::new(), Vec::new()),
        }
    } else {
        (String::new(), Vec::new())
    };

    if !fixed_outfit.is_empty() {
        // 서버 확정 조합 → LLM은 설명만
        let system_prompt = format!(
            r#"너는 코디 해설 AI다. 아래 착장이 왜 어울리는지 설명해라.

착장:
{outfit}
{weather}
규칙:
- 이 착장이 왜 자연스러운지 질감, 톤, 밸런스 관점에서 1~3줄로 설명.
- "추천합니다", "어울립니다" 같은 표현 대신 구체적으로 설명.
- JSON으로만 응답:
{{
  "reason": "설명 1~3줄"
}}"#,
            outfit = fixed_outfit,
            weather = if weather_hint.is_empty() {
                String::new()
            } else {
                format!("\n날씨: {}", weather_hint)
            },
        );

        let response_text = call_openai_chat(
            &state.http_client,
            &state.openai_api_key,
            &system_prompt,
            &body.message,
        )
        .await
        .map_err(AppError::Internal)?;

        let reason = extract_reason(&response_text);
        Ok(Json(ChatResponse {
            reply: reason,
            items: outfit_items,
        }))
    } else {
        // anchor 없는 일반 질문 → 기존 방식 (옷장 기반 자유 응답)
        let wardrobe_summary = build_wardrobe_summary(&clothes);
        let system_prompt = format!(
            r#"너는 아메카지/밀리터리 빈티지/워크웨어 믹스 전문 코디 상담 AI다.

원칙:
1. 강한 아이템 1개 + 나머지 힘 빼기
2. 톤 대비: 밝음/중간/어두움을 섞는다
3. role 밸런스: 밥 위주, 반찬은 1개 이하

규칙:
- 옷장에 있는 아이템만 추천. 이름 정확히 복사.
- 옷장에 마땅한 게 없으면 어떤 아이템이 좋을지 제안 (owned: false).
- JSON으로만 응답:
{{
  "reason": "설명",
  "items": [
    {{ "slot": "inner", "name": "아이템명", "owned": true }},
    {{ "slot": "bottom", "name": "아이템명", "owned": true }},
    {{ "slot": "shoes", "name": "아이템명", "owned": true }},
    {{ "slot": "bag", "name": "아이템명", "owned": true }}
  ]
}}
{weather}
옷장:
{wardrobe}"#,
            wardrobe = wardrobe_summary,
            weather = if weather_hint.is_empty() {
                String::new()
            } else {
                format!("\n날씨: {}\n", weather_hint)
            },
        );

        let response_text = call_openai_chat(
            &state.http_client,
            &state.openai_api_key,
            &system_prompt,
            &body.message,
        )
        .await
        .map_err(AppError::Internal)?;

        let (reply, items) = parse_chat_response(&response_text, &clothes);
        Ok(Json(ChatResponse { reply, items }))
    }
}

// ─── 서버 확정 조합 생성 ───

fn build_final_outfit(
    anchor: &Clothing,
    clothes: &[Clothing],
    user: Option<&crate::models::user_profile::UserStyleProfile>,
    temperature: Option<f64>,
    feedback: &outfit_scorer::FeedbackContext,
) -> Option<(String, Vec<ChatItem>)> {
    let anchor_cat = &anchor.category;
    let temp = temperature.unwrap_or(20.0);

    let slot_candidates = |cat: &str, k: usize| -> Vec<&Clothing> {
        let mut scored: Vec<(&Clothing, i32)> = clothes
            .iter()
            .filter(|c| c.category == cat && c.id != anchor.id)
            .filter(|c| is_weather_appropriate(c, temp))
            .map(|c| (c, outfit_scorer::complement_score(anchor, c)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().take(k).map(|(c, _)| c).collect()
    };

    let tops = if anchor_cat == "상의" { vec![anchor] } else { slot_candidates("상의", 5) };
    let bottoms = if anchor_cat == "하의" { vec![anchor] } else { slot_candidates("하의", 5) };
    let outers_pool = if anchor_cat == "아우터" { vec![anchor] } else { slot_candidates("아우터", 4) };
    let shoes = if anchor_cat == "신발" { vec![anchor] } else { slot_candidates("신발", 4) };
    let bags = if anchor_cat == "가방" { vec![anchor] } else { slot_candidates("가방", 3) };

    // 조합 생성 + outfit_score (아우터 없는 것 + 있는 것)
    let mut combos: Vec<(Vec<&Clothing>, i32)> = Vec::new();

    // 아우터 없는 조합
    for top in &tops {
        for bottom in &bottoms {
            for shoe in &shoes {
                for bag in &bags {
                    let outfit = vec![*top, *bottom, *shoe, *bag];
                    let score = outfit_scorer::total_outfit_score_with_feedback(anchor, &outfit, user, feedback);
                    combos.push((outfit, score));
                }
            }
        }
    }
    // 아우터 있는 조합
    for top in &tops {
        for outer in &outers_pool {
            for bottom in &bottoms {
                for shoe in &shoes {
                    for bag in &bags {
                        let outfit = vec![*top, *outer, *bottom, *shoe, *bag];
                        let score = outfit_scorer::total_outfit_score_with_feedback(anchor, &outfit, user, feedback);
                        combos.push((outfit, score));
                    }
                }
            }
        }
    }

    combos.sort_by(|a, b| b.1.cmp(&a.1));

    let (best_outfit, best_score) = combos.first()?;

    tracing::info!(
        "final outfit (score={}): {}",
        best_score,
        best_outfit.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(" / ")
    );

    // 확정 조합을 텍스트 + ChatItem으로 변환
    let mut desc_parts = Vec::new();
    let mut items = Vec::new();
    for c in best_outfit.iter() {
        let slot = match c.category.as_str() {
            "상의" => "inner",
            "아우터" => "outer",
            "하의" => "bottom",
            "신발" => "shoes",
            "가방" => "bag",
            _ => continue,
        };
        let label = match slot {
            "inner" => "이너",
            "outer" => "아우터",
            "bottom" => "하의",
            "shoes" => "신발",
            "bag" => "가방",
            _ => slot,
        };
        desc_parts.push(format!("{}: {}", label, c.name));
        items.push(ChatItem {
            slot: slot.to_string(),
            category: c.category.clone(),
            name: c.name.clone(),
            owned: true,
        });
    }
    // anchor가 outfit에 없으면 추가 (미보유 아이템일 수 있음)
    if !items.iter().any(|i| i.name == anchor.name) {
        let slot = match anchor.category.as_str() {
            "상의" => "inner",
            "아우터" => "outer",
            "하의" => "bottom",
            "신발" => "shoes",
            "가방" => "bag",
            _ => "?",
        };
        let label = match slot {
            "inner" => "이너",
            "outer" => "아우터",
            "bottom" => "하의",
            "shoes" => "신발",
            "bag" => "가방",
            _ => slot,
        };
        desc_parts.push(format!("{}: {}", label, anchor.name));
        items.push(ChatItem {
            slot: slot.to_string(),
            category: anchor.category.clone(),
            name: anchor.name.clone(),
            owned: clothes.iter().any(|c| c.name == anchor.name),
        });
    }

    Some((desc_parts.join("\n"), items))
}

// ─── Anchor 탐색 ───

fn find_anchors<'a>(message: &str, clothes: &'a [Clothing]) -> Vec<&'a Clothing> {
    let msg = message.to_lowercase();
    let mut found: Vec<&Clothing> = Vec::new();

    for c in clothes {
        if msg.contains(&c.name.to_lowercase()) {
            found.push(c);
        }
    }

    if found.is_empty() {
        let color_keywords: Vec<(&str, &str)> = vec![
            ("네이비 스니커", "네이비"),
            ("올네이비", "네이비"),
            ("블랙 스니커", "블랙"),
            ("화이트 스니커", "화이트"),
            ("브라운 부츠", "브라운"),
            ("올리브", "올리브"),
            ("독일군", "독일군"),
            ("데저트부츠", "데저트"),
            ("워크부츠", "워크부츠"),
        ];
        for (keyword, color_hint) in &color_keywords {
            if msg.contains(*keyword) || msg.contains(&keyword.to_lowercase()) {
                for c in clothes {
                    let name_lower = c.name.to_lowercase();
                    if name_lower.contains(&color_hint.to_lowercase()) {
                        if msg.contains("스니커") && c.category == "신발"
                            || msg.contains("부츠") && c.category == "신발"
                            || msg.contains("셔츠") && c.category == "상의"
                            || msg.contains("팬츠") && c.category == "하의"
                            || msg.contains("자켓") && c.category == "아우터"
                            || msg.contains("가방") && c.category == "가방"
                            || msg.contains("백팩") && c.category == "가방"
                            || !msg.contains("스니커")
                                && !msg.contains("부츠")
                                && !msg.contains("셔츠")
                                && !msg.contains("팬츠")
                                && !msg.contains("자켓")
                        {
                            if !found.iter().any(|f| f.id == c.id) {
                                found.push(c);
                            }
                        }
                    }
                }
                if !found.is_empty() {
                    break;
                }
            }
        }
    }

    found.truncate(3);
    found
}

// ─── 온도 기반 아이템 필터 ───

fn is_weather_appropriate(item: &Clothing, temp: f64) -> bool {
    let weight = item.weight.as_deref().unwrap_or("중간");
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    if temp >= 20.0 {
        if mat == "wool" || mat == "flannel" { return false; }
        if name.contains("니트") && !name.contains("가벼") { return false; }
        if name.contains("울 ") { return false; }
        if item.category == "아우터" && weight == "무거움" { return false; }
        if name.contains("코트") || name.contains("파카") { return false; }
    }
    if temp >= 25.0 {
        if item.category == "아우터" && weight != "가벼움" { return false; }
        if name.contains("코듀로이") { return false; }
        if weight == "무거움" { return false; }
    }
    true
}

// ─── 유틸리티 ───

fn build_wardrobe_summary(clothes: &[Clothing]) -> String {
    let mut sections = Vec::new();
    for cat in &["상의", "하의", "아우터", "신발", "가방"] {
        let items: Vec<String> = clothes
            .iter()
            .filter(|c| c.category == *cat)
            .map(|c| {
                format!(
                    "- {} | role:{} | tone:{} | style:{} | color_temp:{} | weight:{} | texture:{}",
                    c.name,
                    c.role.as_deref().unwrap_or("-"),
                    c.tone.as_deref().unwrap_or("-"),
                    c.style.as_deref().unwrap_or("-"),
                    c.color_temperature.as_deref().unwrap_or("-"),
                    c.visual_weight.as_deref().unwrap_or("-"),
                    c.texture_depth.as_deref().unwrap_or("-"),
                )
            })
            .collect();
        sections.push(format!("[{}]\n{}", cat, items.join("\n")));
    }
    sections.join("\n\n")
}

fn extract_reason(text: &str) -> String {
    if let Some(json_str) = extract_json_block(text) {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(reason) = obj["reason"].as_str() {
                return reason.to_string();
            }
        }
    }
    // JSON 파싱 실패 시 원문 반환
    text.trim().to_string()
}

fn parse_chat_response(text: &str, clothes: &[Clothing]) -> (String, Vec<ChatItem>) {
    let json_str = extract_json_block(text);
    let parsed: Option<serde_json::Value> = json_str.and_then(|s| serde_json::from_str(&s).ok());
    let Some(obj) = parsed else {
        return (text.to_string(), Vec::new());
    };

    let reason = obj["reason"].as_str().unwrap_or("").to_string();
    let slot_to_cat = |s: &str| match s {
        "top" | "inner" => Some("상의"),
        "bottom" => Some("하의"),
        "outer" => Some("아우터"),
        "shoes" => Some("신발"),
        "bag" => Some("가방"),
        _ => None,
    };

    let mut items = Vec::new();
    if let Some(arr) = obj["items"].as_array() {
        for entry in arr {
            let slot = entry["slot"].as_str().unwrap_or("");
            let name = entry["name"].as_str().unwrap_or("").trim();
            let Some(category) = slot_to_cat(slot) else { continue };
            if name.is_empty() { continue; }
            let actually_owned = clothes.iter().any(|c| c.name == name && c.category == category);
            items.push(ChatItem {
                slot: slot.to_string(),
                category: category.to_string(),
                name: name.to_string(),
                owned: actually_owned,
            });
        }
    }
    (reason, items)
}

fn extract_json_block(text: &str) -> Option<String> {
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            let block = after[..end].trim();
            if block.starts_with('{') {
                return Some(block.to_string());
            }
        }
    }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return Some(text[start..=end].to_string());
        }
    }
    None
}

async fn call_openai_chat(
    client: &reqwest::Client,
    api_key: &str,
    system_prompt: &str,
    user_message: &str,
) -> anyhow::Result<String> {
    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_message }
        ],
        "temperature": 0.5,
        "max_tokens": 500,
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        return Err(anyhow::anyhow!("OpenAI error {}: {}", status, text));
    }

    let json: serde_json::Value = serde_json::from_str(&text)?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(content)
}
