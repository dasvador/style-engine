use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::db::clothing_repo;
use crate::errors::AppError;
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
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
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

    // anchor 아이템 탐색 — 유저 메시지에서 옷장 아이템 이름 매칭
    let anchors = find_anchors(&body.message, &clothes);

    // 유저 프로파일 로드
    let user_profile = sqlx::query_as::<_, crate::models::user_profile::UserStyleProfile>(
        "SELECT user_id, height_cm, weight_kg, upper_body, calves, preferred_fit, needs_grounded_shoes, prefers_weighted_bag FROM user_style_profile WHERE user_id = 'default'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    // anchor 기반 조합 후보 생성 (outfit_scorer 3계층)
    let outfit_hint = if anchors.is_empty() {
        String::new()
    } else {
        build_outfit_candidates(anchors[0], &clothes, user_profile.as_ref(), temperature)
    };

    let wardrobe_summary = build_wardrobe_summary(&clothes);

    let system_prompt = format!(
        r#"너는 아메카지/밀리터리 빈티지/워크웨어 믹스 전문 코디 상담 AI다.

원칙:
1. 강한 아이템 1개 + 나머지 힘 빼기
2. 톤 대비: 밝음/중간/어두움을 섞는다
3. role 밸런스: 밥 위주, 반찬은 1개 이하

규칙:
- 서버가 추천 조합을 제시하면, 반드시 그 중에서 하나를 선택하라. 자체 조합을 만들지 마라.
- 서버 조합이 없을 때만 옷장에서 직접 선택 가능.
- 옷장에 마땅한 아이템이 없으면 어떤 아이템이 좋을지 제안 (owned: false).
- 사용자가 지정한 아이템은 반드시 그대로 포함. 교체 금지.
- 반드시 아래 JSON 형식으로만 응답. 다른 텍스트 금지.

응답 JSON:
{{
  "reason": "이 조합이 좋은 이유 1~2줄",
  "items": [
    {{ "slot": "inner", "name": "이너 아이템명", "owned": true }},
    {{ "slot": "outer", "name": "아우터 아이템명", "owned": true }},
    {{ "slot": "bottom", "name": "하의 아이템명", "owned": true }},
    {{ "slot": "shoes", "name": "신발 아이템명", "owned": true }},
    {{ "slot": "bag", "name": "가방 아이템명", "owned": true }}
  ]
}}
- inner = 상의(이너). outer = 아우터. 구분해서 추천.
- bag은 항상 포함.
- owned: true = 옷장에 있는 아이템, false = 옷장에 없지만 추천하는 아이템.
- 아우터가 불필요한 날씨면 outer를 생략 가능. 나머지 슬롯은 항상 포함.
{weather_section}{outfit_section}
옷장:
{wardrobe}"#,
        wardrobe = wardrobe_summary,
        weather_section = if weather_hint.is_empty() {
            String::new()
        } else {
            format!("\n날씨: {}\n", weather_hint)
        },
        outfit_section = if outfit_hint.is_empty() {
            String::new()
        } else {
            format!(
                "\n아래 조합 중 하나를 반드시 선택하라. 자체 조합을 만들지 마라:\n{}\n",
                outfit_hint
            )
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

// ─── Anchor 탐색: 유저 메시지에서 옷장 아이템 이름/키워드 매칭 ───

fn find_anchors<'a>(message: &str, clothes: &'a [Clothing]) -> Vec<&'a Clothing> {
    let msg = message.to_lowercase();
    let mut found: Vec<&Clothing> = Vec::new();

    // 1. 정확한 아이템명 매칭
    for c in clothes {
        if msg.contains(&c.name.to_lowercase()) {
            found.push(c);
        }
    }

    // 2. 부분 키워드 매칭 (정확한 매칭이 없을 때)
    if found.is_empty() {
        // 색상+카테고리 키워드로 매칭
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
                // 해당 색상이 포함된 아이템 찾기
                for c in clothes {
                    let name_lower = c.name.to_lowercase();
                    if name_lower.contains(&color_hint.to_lowercase()) {
                        // 카테고리 힌트도 체크
                        if msg.contains("스니커") && c.category == "신발"
                            || msg.contains("부츠") && c.category == "신발"
                            || msg.contains("셔츠") && c.category == "상의"
                            || msg.contains("팬츠") && c.category == "하의"
                            || msg.contains("자켓") && c.category == "아우터"
                            || msg.contains("가방") && c.category == "가방"
                            || msg.contains("백팩") && c.category == "가방"
                            // 일반적 색상 매칭
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

    found.truncate(3); // 최대 3개
    found
}

// ─── 조합 후보 생성 + outfit_score 순위 ───

fn build_outfit_candidates(
    anchor: &Clothing,
    clothes: &[Clothing],
    user: Option<&crate::models::user_profile::UserStyleProfile>,
    temperature: Option<f64>,
) -> String {
    let anchor_cat = &anchor.category;
    let temp = temperature.unwrap_or(20.0);

    // 슬롯별 item-level top-k 추출 (온도 기반 필터 포함)
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
    let outers = if anchor_cat == "아우터" { vec![anchor] } else { slot_candidates("아우터", 4) };
    let shoes = if anchor_cat == "신발" { vec![anchor] } else { slot_candidates("신발", 4) };
    let bags = if anchor_cat == "가방" { vec![anchor] } else { slot_candidates("가방", 3) };

    // 조합 생성 + outfit_score
    let mut combos: Vec<(Vec<&Clothing>, i32)> = Vec::new();

    for top in &tops {
        for bottom in &bottoms {
            for shoe in &shoes {
                for bag in &bags {
                    let outfit: Vec<&Clothing> = vec![*top, *bottom, *shoe, *bag];
                    let score = outfit_scorer::total_outfit_score(anchor, &outfit, user);
                    combos.push((outfit, score));
                }
            }
        }
    }

    // 아우터 포함 조합도 추가 (상위 후보만)
    for top in &tops {
        for bottom in &bottoms {
            for outer in &outers {
                for shoe in &shoes {
                    for bag in &bags {
                        let outfit: Vec<&Clothing> = vec![*top, *outer, *bottom, *shoe, *bag];
                        let score = outfit_scorer::total_outfit_score(anchor, &outfit, user);
                        combos.push((outfit, score));
                    }
                }
            }
        }
    }

    // 상위 3개 조합 선정
    combos.sort_by(|a, b| b.1.cmp(&a.1));
    combos.truncate(3);

    if combos.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    for (i, (outfit, _score)) in combos.iter().enumerate() {
        let mut parts = Vec::new();
        for c in outfit.iter() {
            let slot = match c.category.as_str() {
                "상의" => "inner",
                "하의" => "bottom",
                "아우터" => "outer",
                "신발" => "shoes",
                "가방" => "bag",
                _ => continue,
            };
            parts.push(format!("\"{}\":\"{}\"", slot, c.name));
        }
        lines.push(format!("조합{}: {{{}}}", i + 1, parts.join(",")));
    }

    lines.join("\n")
}

// ─── 온도 기반 아이템 필터 ───

fn is_weather_appropriate(item: &Clothing, temp: f64) -> bool {
    let weight = item.weight.as_deref().unwrap_or("중간");
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    // 20도 이상: 울/니트/무거운 아우터 제외
    if temp >= 20.0 {
        if mat == "wool" || mat == "flannel" { return false; }
        if name.contains("니트") && !name.contains("가벼") { return false; }
        if name.contains("울 ") { return false; }
        if item.category == "아우터" && weight == "무거움" { return false; }
        if name.contains("코트") || name.contains("파카") { return false; }
    }

    // 25도 이상: 아우터 대부분 제외, 가벼운 것만
    if temp >= 25.0 {
        if item.category == "아우터" && weight != "가벼움" { return false; }
        if name.contains("코듀로이") { return false; }
        if weight == "무거움" { return false; }
    }

    // 10도 이하: 린넨/가벼운 반팔 단독 비추 (필터까지는 아니고 shortlist에서 하향)
    // → 이건 scoring에서 처리

    true
}

// ─── 나머지 (옷장 요약, 파싱, OpenAI 호출) ───

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

fn parse_chat_response(text: &str, clothes: &[Clothing]) -> (String, Vec<ChatItem>) {
    let json_str = extract_json_block(text);

    let parsed: Option<serde_json::Value> = json_str
        .and_then(|s| serde_json::from_str(&s).ok());

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

    // 배열 형식: items: [{slot, name, owned}]
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
    // 이전 형식 호환: items: {top/inner: "", bottom: "", ...}
    else if obj["items"].is_object() {
        for (key, cat) in [("inner","상의"),("top","상의"),("bottom","하의"),("outer","아우터"),("shoes","신발"),("bag","가방")] {
            if let Some(name) = obj["items"][key].as_str() {
                let name = name.trim();
                if name.is_empty() { continue; }
                let actually_owned = clothes.iter().any(|c| c.name == name && c.category == cat);
                items.push(ChatItem {
                    slot: key.to_string(),
                    category: cat.to_string(),
                    name: name.to_string(),
                    owned: actually_owned,
                });
            }
        }
    }

    (reason, items)
}

fn extract_json_block(text: &str) -> Option<String> {
    // ```json ... ``` 블록
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
    // raw JSON
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
        "max_tokens": 1000,
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
