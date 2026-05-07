use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{clothing_repo, feedback_repo};
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::models::feedback::FeedbackRequest;
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

#[derive(Debug, Serialize, Clone)]
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
    if state.openai_api_key.is_empty() || state.openai_api_key == "sk-your-key-here" {
        return Err(AppError::BadRequest("OPENAI_API_KEY is not configured".to_string()));
    }

    let user_id = &auth.user_id;
    let clothes = clothing_repo::list_clothing(&state.db).await?;

    // 날씨
    let mut temperature: Option<f64> = None;
    let weather_hint = match crate::db::region_repo::get_region(&state.db).await {
        Ok(Some(region)) => {
            match weather_service::fetch_weather(&state.http_client, &state.kma_api_key, region.latitude, region.longitude).await {
                Ok(w) => { temperature = Some(w.temperature); format!("{}°C, {}", w.temperature, w.weather_description) }
                Err(_) => String::new(),
            }
        }
        _ => String::new(),
    };

    // 유저 프로파일
    let user_profile = sqlx::query_as::<_, crate::models::user_profile::UserStyleProfile>(
        "SELECT * FROM user_style_profile WHERE user_id = ?"
    ).bind(user_id).fetch_optional(&state.db).await.ok().flatten();

    // 피드백
    let feedback_ctx = {
        let item_scores = feedback_repo::get_item_adjustments(&state.db, user_id).await.unwrap_or_default();
        let pref_scores = feedback_repo::get_preference_scores(&state.db, user_id).await.unwrap_or_default();
        outfit_scorer::FeedbackContext {
            item_adj: item_scores.into_iter().map(|s| (s.item_name, s.score_adjustment)).collect(),
            preference: pref_scores.into_iter().map(|s| (s.reason_tag, s.score)).collect(),
        }
    };

    // ─── Tool definitions ───
    let tools = json!([
        {
            "type": "function",
            "function": {
                "name": "search_wardrobe",
                "description": "유저 옷장에서 아이템을 자연어로 검색한다. anchor 아이템을 찾을 때 사용.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "검색할 아이템 설명 (예: 올네이비 스니커, 모카브라운 워크자켓)" }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_outfit",
                "description": "anchor 아이템 기준으로 서버가 최적의 착장을 생성한다. 서버가 scoring/penalty/body balance를 적용해 최종 착장을 확정한다.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "anchor_name": { "type": "string", "description": "anchor 아이템 이름 (search_wardrobe 결과에서 선택)" },
                        "avoid_tags": { "type": "array", "items": { "type": "string" }, "description": "피할 스타일 태그 (예: too_military, too_dark)" }
                    },
                    "required": ["anchor_name"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "submit_feedback",
                "description": "유저가 대화 중 표현한 선호/비선호를 저장한다.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "feedback_type": { "type": "string", "enum": ["like", "dislike"], "description": "좋아요/싫어요" },
                        "reason_tags": { "type": "array", "items": { "type": "string" }, "description": "이유 태그 (too_military, good_texture 등)" },
                        "comment": { "type": "string", "description": "유저 원문 피드백" }
                    },
                    "required": ["feedback_type"]
                }
            }
        }
    ]);

    let system_prompt = format!(
        r#"너는 아메카지/밀리터리 빈티지/워크웨어 믹스 전문 코디 상담 AI다.

역할:
- 유저의 질문을 이해하고, 도구를 호출해서 답변한다.
- 코디 추천은 반드시 get_outfit 도구를 통해 서버가 결정한다. 직접 아이템을 고르지 마라.
- anchor 아이템을 찾을 때는 search_wardrobe를 호출한다.
- 유저가 싫다/좋다 등 피드백을 주면 submit_feedback을 호출한다.

흐름:
1. 유저가 아이템을 언급하면 → search_wardrobe로 anchor 찾기
2. anchor가 확정되면 → get_outfit으로 서버 추천 받기
3. 결과를 자연스럽게 설명
4. 유저가 피드백 주면 → submit_feedback 후 get_outfit 재호출

날씨: {weather}
답변은 한국어로."#,
        weather = if weather_hint.is_empty() { "정보 없음".to_string() } else { weather_hint.clone() },
    );

    // ─── Tool calling loop (최대 5회 반복) ───
    let mut messages = vec![
        json!({"role": "system", "content": system_prompt}),
        json!({"role": "user", "content": body.message}),
    ];
    let mut final_items: Vec<ChatItem> = Vec::new();
    let mut final_reply = String::new();

    for _turn in 0..5 {
        let req_body = json!({
            "model": "gpt-4o-mini",
            "messages": messages,
            "tools": tools,
            "temperature": 0.5,
            "max_tokens": 1000,
        });

        let resp = state.http_client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", state.openai_api_key))
            .json(&req_body)
            .send().await
            .map_err(|e| AppError::Internal(e.into()))?;

        let resp_json: serde_json::Value = resp.json().await
            .map_err(|e| AppError::Internal(e.into()))?;

        let choice = &resp_json["choices"][0];
        let finish = choice["finish_reason"].as_str().unwrap_or("");
        let msg = &choice["message"];

        // 응답 메시지를 히스토리에 추가
        messages.push(msg.clone());

        if finish == "tool_calls" {
            let empty_arr = vec![];
            let tool_calls = msg["tool_calls"].as_array().unwrap_or(&empty_arr);
            for tc in tool_calls {
                let fn_name = tc["function"]["name"].as_str().unwrap_or("");
                let fn_args: serde_json::Value = serde_json::from_str(
                    tc["function"]["arguments"].as_str().unwrap_or("{}")
                ).unwrap_or(json!({}));
                let tc_id = tc["id"].as_str().unwrap_or("");

                let result = match fn_name {
                    "search_wardrobe" => {
                        let query = fn_args["query"].as_str().unwrap_or("");
                        tool_search_wardrobe(query, &clothes)
                    }
                    "get_outfit" => {
                        let anchor_name = fn_args["anchor_name"].as_str().unwrap_or("");
                        let avoid: Vec<String> = fn_args["avoid_tags"].as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let (outfit_json, items) = tool_get_outfit(
                            anchor_name, &clothes, user_profile.as_ref(),
                            temperature, &feedback_ctx,
                        );
                        final_items = items;
                        outfit_json
                    }
                    "submit_feedback" => {
                        let fb_type = fn_args["feedback_type"].as_str().unwrap_or("dislike");
                        let reasons: Vec<String> = fn_args["reason_tags"].as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let comment = fn_args["comment"].as_str().map(String::from);

                        let fb_req = FeedbackRequest {
                            feedback_type: fb_type.to_string(),
                            reasons,
                            inner_name: final_items.iter().find(|i| i.slot == "inner").map(|i| i.name.clone()),
                            outer_name: final_items.iter().find(|i| i.slot == "outer").map(|i| i.name.clone()),
                            bottom_name: final_items.iter().find(|i| i.slot == "bottom").map(|i| i.name.clone()),
                            shoes_name: final_items.iter().find(|i| i.slot == "shoes").map(|i| i.name.clone()),
                            bag_name: final_items.iter().find(|i| i.slot == "bag").map(|i| i.name.clone()),
                            anchor_name: None,
                            comment,
                        };
                        let _ = feedback_repo::insert_feedback(&state.db, user_id, &fb_req).await;
                        json!({"status": "saved"}).to_string()
                    }
                    _ => json!({"error": "unknown tool"}).to_string(),
                };

                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tc_id,
                    "content": result,
                }));
            }
        } else {
            // finish_reason == "stop" → 최종 응답
            final_reply = msg["content"].as_str().unwrap_or("").to_string();
            break;
        }
    }

    Ok(Json(ChatResponse {
        reply: final_reply,
        items: final_items,
    }))
}

// ─── Tool implementations ───

fn tool_search_wardrobe(query: &str, clothes: &[Clothing]) -> String {
    let q = query.to_lowercase();
    let mut results: Vec<serde_json::Value> = Vec::new();

    for c in clothes {
        let name_lower = c.name.to_lowercase();
        let color = c.color.as_deref().unwrap_or("").to_lowercase();

        // 이름 포함 매칭
        let name_match = q.split_whitespace().all(|word| {
            name_lower.contains(word) || color.contains(word)
        });
        // 부분 매칭
        let partial = q.split_whitespace().any(|word| {
            name_lower.contains(word) || color.contains(word)
        });

        if name_match {
            results.push(json!({"name": c.name, "category": c.category, "confidence": 0.95}));
        } else if partial {
            results.push(json!({"name": c.name, "category": c.category, "confidence": 0.6}));
        }
    }

    results.sort_by(|a, b| {
        b["confidence"].as_f64().unwrap_or(0.0)
            .partial_cmp(&a["confidence"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(5);

    serde_json::to_string(&results).unwrap_or("[]".to_string())
}

fn tool_get_outfit(
    anchor_name: &str,
    clothes: &[Clothing],
    user: Option<&crate::models::user_profile::UserStyleProfile>,
    temperature: Option<f64>,
    feedback: &outfit_scorer::FeedbackContext,
) -> (String, Vec<ChatItem>) {
    let anchor = match clothes.iter().find(|c| c.name == anchor_name) {
        Some(a) => a,
        None => return (json!({"error": "anchor not found"}).to_string(), Vec::new()),
    };

    let result = build_final_outfit(anchor, clothes, user, temperature, feedback);
    match result {
        Some((desc, items)) => {
            let items_json: Vec<serde_json::Value> = items.iter().map(|i| {
                json!({"slot": i.slot, "name": i.name, "category": i.category, "owned": i.owned})
            }).collect();
            let response = json!({
                "outfit": desc,
                "items": items_json,
                "note": "서버가 확정한 착장입니다. 아이템을 변경하지 마세요."
            });
            (response.to_string(), items)
        }
        None => (json!({"error": "no suitable outfit found"}).to_string(), Vec::new()),
    }
}

// ─── 서버 확정 조합 생성 (기존 로직 유지) ───

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
        let mut scored: Vec<(&Clothing, i32)> = clothes.iter()
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

    let mut combos: Vec<(Vec<&Clothing>, i32)> = Vec::new();

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

    tracing::info!("final outfit (score={}): {}", best_score,
        best_outfit.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(" / "));

    let mut desc_parts = Vec::new();
    let mut items = Vec::new();
    for c in best_outfit.iter() {
        let slot = match c.category.as_str() {
            "상의" => "inner", "아우터" => "outer", "하의" => "bottom",
            "신발" => "shoes", "가방" => "bag", _ => continue,
        };
        desc_parts.push(format!("{}: {}", slot, c.name));
        items.push(ChatItem { slot: slot.to_string(), category: c.category.clone(), name: c.name.clone(), owned: true });
    }
    if !items.iter().any(|i| i.name == anchor.name) {
        let slot = match anchor.category.as_str() {
            "상의" => "inner", "아우터" => "outer", "하의" => "bottom",
            "신발" => "shoes", "가방" => "bag", _ => "?",
        };
        desc_parts.push(format!("{}: {}", slot, anchor.name));
        items.push(ChatItem {
            slot: slot.to_string(), category: anchor.category.clone(),
            name: anchor.name.clone(), owned: clothes.iter().any(|c| c.name == anchor.name),
        });
    }
    Some((desc_parts.join("\n"), items))
}

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
