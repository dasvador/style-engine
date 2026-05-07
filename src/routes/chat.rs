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
    Router::new()
        .route("/", post(chat))
        .route("/image", post(generate_image))
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
                "description": "유저 옷장에서 아이템을 자연어로 검색한다. 유저가 말한 아이템이 어떤 카테고리인지 판단해서 category를 함께 넘겨라.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "검색할 아이템 설명 (예: 올리브 슬립온, 모카브라운 워크자켓)" },
                        "category": { "type": "string", "enum": ["상의","하의","아우터","신발","가방"], "description": "아이템의 카테고리. 슬립온/스니커/부츠/샌들→신발, 자켓/코트/가디건→아우터, 팬츠/데님→하의 등" }
                    },
                    "required": ["query", "category"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_outfit",
                "description": "anchor 아이템 기준으로 서버가 최적의 착장을 생성한다. user_query는 유저 원문, anchor_name은 search_wardrobe에서 찾은 정확한 DB 이름.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "user_query": { "type": "string", "description": "유저가 언급한 원래 아이템 표현 (예: 올리브 슬립온)" },
                        "anchor_name": { "type": "string", "description": "search_wardrobe 결과에서 선택한 정확한 DB 이름" },
                        "avoid_tags": { "type": "array", "items": { "type": "string" }, "description": "피할 스타일 태그" }
                    },
                    "required": ["user_query", "anchor_name"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "evaluate_outfit",
                "description": "서버가 착장 조합의 품질을 검증한다. 문제가 있으면 이유와 함께 실패를 반환한다. get_outfit 결과를 검증할 때 사용.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_names": { "type": "array", "items": { "type": "string" }, "description": "검증할 아이템 이름 목록" }
                    },
                    "required": ["item_names"]
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
3. get_outfit 결과를 evaluate_outfit으로 검증
4. 검증 통과 → 결과를 자연스럽게 설명
5. 검증 실패 → get_outfit을 avoid_tags와 함께 재호출
6. 유저가 피드백 주면 → submit_feedback 후 get_outfit 재호출

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
    let mut first_search_query: Option<String> = None; // 유저 최초 검색어 (덮어쓰기 불가)
    let mut anchor_category: Option<String> = None;

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
                tracing::info!("tool_call: {}({})", fn_name, fn_args);

                let result = match fn_name {
                    "search_wardrobe" => {
                        let query = fn_args["query"].as_str().unwrap_or("");
                        let category = fn_args["category"].as_str();
                        // 유저 최초 검색어만 저장 (두 번째 호출로 덮어쓰기 방지)
                        if first_search_query.is_none() {
                            first_search_query = Some(query.to_string());
                            anchor_category = category.map(|c| c.to_string());
                        }
                        let result = tool_search_wardrobe(query, category, &clothes, &state.embedding);
                        tracing::info!("search_wardrobe(cat={:?}): {}", category, &result[..result.len().min(300)]);
                        result
                    }
                    "get_outfit" => {
                        let user_query = fn_args["user_query"].as_str().unwrap_or(
                            fn_args["anchor_name"].as_str().unwrap_or("")
                        );
                        let anchor_name = fn_args["anchor_name"].as_str().unwrap_or(user_query);
                        let (outfit_json, mut items) = tool_get_outfit(
                            user_query, anchor_name, &clothes, user_profile.as_ref(),
                            temperature, &feedback_ctx, &state.embedding,
                        );
                        // 유저 원문으로 anchor 슬롯 즉시 교체
                        if let Some(ref uq) = first_search_query {
                            let is_in_db = clothes.iter().any(|c| c.name == *uq);
                            if !is_in_db {
                                if let Some(ref cat) = anchor_category {
                                    let sk = match cat.as_str() {
                                        "신발" => "shoes", "아우터" => "outer", "하의" => "bottom",
                                        "가방" => "bag", "상의" => "inner", _ => "",
                                    };
                                    if let Some(item) = items.iter_mut().find(|i| i.slot == sk) {
                                        item.name = uq.clone();
                                        item.owned = false;
                                    }
                                }
                            }
                        }
                        final_items = items;
                        outfit_json
                    }
                    "evaluate_outfit" => {
                        let names: Vec<String> = fn_args["item_names"].as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        tool_evaluate_outfit(&names, &clothes, user_profile.as_ref())
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

            // 유저 원문이 DB에 없으면 anchor 슬롯을 원문으로 교체
            if let Some(ref uq) = first_search_query {
                let is_in_db = clothes.iter().any(|c| c.name == *uq);
                tracing::info!("anchor override check: query='{}' in_db={} cat={:?} items={}", uq, is_in_db, anchor_category, final_items.len());
                if !is_in_db {
                    let slot_key = match anchor_category.as_deref() {
                        Some("신발") => "shoes",
                        Some("아우터") => "outer",
                        Some("하의") => "bottom",
                        Some("가방") => "bag",
                        Some("상의") => "inner",
                        _ => "",
                    };
                    if !slot_key.is_empty() {
                        if let Some(item) = final_items.iter_mut().find(|i| i.slot == slot_key) {
                            tracing::info!("anchor override: {} '{}' → '{}'", slot_key, item.name, uq);
                            item.name = uq.clone();
                            item.owned = false;
                        } else {
                            tracing::warn!("anchor override: slot '{}' not found in final_items", slot_key);
                        }
                    }
                }
            }

            break;
        }
    }

    Ok(Json(ChatResponse {
        reply: final_reply,
        items: final_items,
    }))
}

// ─── Tool implementations ───

// ─── AI 이미지 생성 ───

#[derive(Debug, Deserialize)]
struct ImageRequest {
    items: String,
}

#[derive(Debug, Serialize)]
struct ImageResponse {
    image_url: Option<String>,
}

async fn generate_image(
    State(state): State<AppState>,
    Json(body): Json<ImageRequest>,
) -> Result<Json<ImageResponse>, AppError> {
    let prompt = format!(
        r#"Full-body fashion editorial photograph of a man wearing: {}

Style: Japanese premium lookbook, Tokyo street editorial, relaxed natural pose.
Body: broad shoulders, slightly stocky, relaxed XL fit.
Photo: cinematic natural lighting, soft shadows, muted warm tones, shallow depth of field.
Aesthetic: AURALEE / BEAMS / vintage military-workwear mix.
Background: minimal urban, soft bokeh.
NOT: ecommerce, flat lay, mannequin, AI collage, hyper-stylized, cyberpunk."#,
        body.items
    );

    let req_body = json!({
        "model": "gpt-image-1",
        "prompt": prompt,
        "n": 1,
        "size": "1024x1536",
        "quality": "low",
        "output_format": "url",
    });

    let resp = state.http_client
        .post("https://api.openai.com/v1/images/generations")
        .header("Authorization", format!("Bearer {}", state.openai_api_key))
        .json(&req_body)
        .send().await
        .map_err(|e| AppError::Internal(e.into()))?;

    let resp_text = resp.text().await
        .map_err(|e| AppError::Internal(e.into()))?;
    if resp_text.len() < 1000 {
        tracing::info!("image API response: {}", resp_text);
    } else {
        tracing::info!("image API response len={}, has_url={}", resp_text.len(), resp_text.contains("\"url\""));
    }
    let resp_json: serde_json::Value = serde_json::from_str(&resp_text)
        .map_err(|e| AppError::Internal(e.into()))?;

    let url = if let Some(u) = resp_json["data"][0]["url"].as_str() {
        Some(u.to_string())
    } else if let Some(b64) = resp_json["data"][0]["b64_json"].as_str() {
        Some(format!("data:image/png;base64,{}", b64))
    } else {
        None
    };

    Ok(Json(ImageResponse { image_url: url }))
}

// ─── Tool implementations ───

fn tool_search_wardrobe(
    query: &str,
    category: Option<&str>,
    clothes: &[Clothing],
    embedding: &std::sync::Arc<crate::services::embedding::EmbeddingService>,
) -> String {
    // LLM이 판단한 카테고리로 필터 (없으면 전체 검색)
    let search_clothes: Vec<Clothing> = if let Some(cat) = category {
        clothes.iter().filter(|c| c.category == cat).cloned().collect()
    } else {
        clothes.to_vec()
    };

    // 임베딩 기반 시맨틱 검색
    match embedding.search_wardrobe(query, &search_clothes, 5) {
        Ok(matches) => {
            let results: Vec<serde_json::Value> = matches.iter().map(|m| {
                json!({
                    "name": m.name,
                    "category": m.category,
                    "confidence": (m.similarity * 100.0).round() / 100.0,
                })
            }).collect();
            serde_json::to_string(&results).unwrap_or("[]".to_string())
        }
        Err(e) => {
            tracing::warn!("embedding search failed: {e}, falling back to keyword");
            // 폴백: 키워드 매칭
            let q = query.to_lowercase();
            let mut results: Vec<serde_json::Value> = Vec::new();
            for c in clothes {
                let name_lower = c.name.to_lowercase();
                if q.split_whitespace().any(|w| name_lower.contains(w)) {
                    results.push(json!({"name": c.name, "category": c.category, "confidence": 0.6}));
                }
            }
            results.truncate(5);
            serde_json::to_string(&results).unwrap_or("[]".to_string())
        }
    }
}

fn tool_get_outfit(
    user_query: &str,
    anchor_name: &str,
    clothes: &[Clothing],
    user: Option<&crate::models::user_profile::UserStyleProfile>,
    temperature: Option<f64>,
    feedback: &outfit_scorer::FeedbackContext,
    embedding: &std::sync::Arc<crate::services::embedding::EmbeddingService>,
) -> (String, Vec<ChatItem>) {
    let cat_hint_str = extract_category_from_wardrobe(anchor_name, clothes);
    let cat_hint = cat_hint_str.as_deref();

    // anchor 탐색: 정확 → fuzzy → 임베딩. 못 찾아도 유저 원문 유지.
    let exact = clothes.iter().find(|c| c.name == anchor_name);
    let q = anchor_name.to_lowercase();
    let fuzzy = || clothes.iter()
        .filter(|c| cat_hint.map_or(true, |cat| c.category == cat))
        .find(|c| {
            let n = c.name.to_lowercase();
            q.split_whitespace().filter(|w| *w != "신발" && *w != "색").all(|w| n.contains(w))
        });
    let emb_match = || {
        let filtered: Vec<Clothing> = clothes.iter()
            .filter(|c| cat_hint.map_or(true, |cat| c.category == cat))
            .cloned().collect();
        embedding.search_wardrobe(anchor_name, &filtered, 1).ok()
            .and_then(|m| m.into_iter().next())
            .filter(|m| m.similarity > 0.5)
            .and_then(|m| clothes.iter().find(|c| c.name == m.name))
    };

    // scoring용 proxy anchor (DB에서 가장 유사한 아이템)
    let proxy_anchor = exact.or_else(fuzzy).or_else(emb_match);
    let anchor_owned = exact.is_some(); // 정확히 DB에 있는 경우만 owned
    let display_anchor_name = user_query.to_string(); // 항상 유저 원문 유지
    let display_anchor_cat = cat_hint.unwrap_or("상의");

    if let Some(pa) = proxy_anchor {
        tracing::info!("get_outfit: proxy anchor='{}' for query='{}'", pa.name, anchor_name);
    } else {
        tracing::info!("get_outfit: no DB match for '{}', using as unowned anchor", anchor_name);
    }

    // proxy anchor로 조합 생성 (DB에 없어도 유사 아이템 기준으로 scoring)
    let scoring_anchor = match proxy_anchor {
        Some(pa) => pa,
        None => {
            // DB에 유사 아이템도 없으면 첫 번째 아이템 기준으로 폴백
            match clothes.first() {
                Some(c) => c,
                None => return (json!({"error": "wardrobe empty"}).to_string(), Vec::new()),
            }
        }
    };

    let result = build_final_outfit(scoring_anchor, clothes, user, temperature, feedback);
    match result {
        Some((_desc, mut items)) => {
            // anchor 슬롯을 유저 원문으로 교체 (DB에 없어도 원문 유지)
            let anchor_slot = display_anchor_cat;
            let slot_key = match anchor_slot {
                "상의" => "inner", "아우터" => "outer", "하의" => "bottom",
                "신발" => "shoes", "가방" => "bag", _ => "shoes",
            };

            // proxy anchor가 있으면 해당 슬롯의 아이템을 유저 원문으로 덮어쓰기
            if let Some(item) = items.iter_mut().find(|i| i.slot == slot_key) {
                if !anchor_owned {
                    item.name = display_anchor_name.clone();
                    item.owned = false;
                }
            } else {
                // anchor 슬롯이 결과에 없으면 추가
                items.push(ChatItem {
                    slot: slot_key.to_string(),
                    category: anchor_slot.to_string(),
                    name: display_anchor_name.clone(),
                    owned: anchor_owned,
                });
            }

            let desc = items.iter().map(|i| format!("{}: {}", i.slot, i.name)).collect::<Vec<_>>().join("\n");
            let items_json: Vec<serde_json::Value> = items.iter().map(|i| {
                json!({"slot": i.slot, "name": i.name, "category": i.category, "owned": i.owned})
            }).collect();
            let response = json!({ "outfit": desc, "items": items_json });
            (response.to_string(), items)
        }
        None => (json!({"error": "no suitable outfit found"}).to_string(), Vec::new()),
    }
}

fn tool_evaluate_outfit(
    names: &[String],
    clothes: &[Clothing],
    user: Option<&crate::models::user_profile::UserStyleProfile>,
) -> String {
    let items: Vec<&Clothing> = names.iter()
        .filter_map(|n| clothes.iter().find(|c| c.name == *n))
        .collect();

    if items.len() < 2 {
        return json!({"pass": false, "issues": ["아이템을 2개 이상 찾을 수 없습니다"]}).to_string();
    }

    let mut issues: Vec<String> = Vec::new();

    // military/workwear 과밀
    let strong_count = items.iter()
        .filter(|i| i.strong_style_score.unwrap_or(1) >= 5)
        .count();
    if strong_count >= 3 { issues.push("too_military: 강한 스타일 아이템이 3개 이상".to_string()); }

    // 같은 색상군 3+
    let mut cg_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for i in &items {
        let cg = outfit_scorer::color_group(i.color.as_deref().unwrap_or(""));
        if cg != "other" { *cg_counts.entry(cg).or_insert(0) += 1; }
    }
    for (cg, count) in &cg_counts {
        if *count >= 3 { issues.push(format!("color_repetition: {} 색상이 {}개 반복", cg, count)); }
    }

    // 전부 어두움
    let dark_count = items.iter().filter(|i| i.tone.as_deref() == Some("어두움")).count();
    if dark_count >= 3 { issues.push("too_dark: 어두운 톤이 3개 이상".to_string()); }

    // floating
    let avg_float: f32 = items.iter()
        .filter_map(|i| i.floating_score)
        .map(|f| f as f32)
        .sum::<f32>() / items.len().max(1) as f32;
    if avg_float >= 5.0 { issues.push("floating_balance: 전체적으로 떠보임".to_string()); }

    // texture 단조
    let avg_tex: f32 = items.iter()
        .filter_map(|i| i.texture_depth_v2)
        .map(|t| t as f32)
        .sum::<f32>() / items.len().max(1) as f32;
    if avg_tex < 2.5 { issues.push("too_flat: 질감이 너무 밋밋함".to_string()); }

    // grounding 부족 (신발+가방)
    let grounding: i32 = items.iter()
        .filter(|i| i.category == "신발" || i.category == "가방")
        .filter_map(|i| i.grounding_score)
        .map(|g| g as i32)
        .sum();
    if grounding <= 3 { issues.push("low_grounding: 접지감 부족".to_string()); }

    let pass = issues.is_empty();
    json!({"pass": pass, "issues": issues, "score_summary": {
        "strong_style_count": strong_count,
        "dark_tone_count": dark_count,
        "avg_floating": avg_float,
        "avg_texture": avg_tex,
        "grounding": grounding,
    }}).to_string()
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

    // sub_category 다양성 보장: 같은 sub_category에서 최대 2개만
    let slot_candidates = |cat: &str, k: usize| -> Vec<&Clothing> {
        let mut scored: Vec<(&Clothing, i32)> = clothes.iter()
            .filter(|c| c.category == cat && c.id != anchor.id)
            .filter(|c| is_weather_appropriate(c, temp))
            .map(|c| (c, outfit_scorer::complement_score(anchor, c)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));

        let mut result = Vec::new();
        let mut sub_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (c, _) in &scored {
            let sub = c.sub_category.as_deref().unwrap_or("other").to_string();
            let count = sub_counts.entry(sub).or_insert(0);
            if *count < 2 { // 같은 sub_category 최대 2개
                result.push(*c);
                *count += 1;
            }
            if result.len() >= k { break; }
        }
        result
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

/// DB 아이템의 이름/sub_category와 매칭해서 카테고리를 동적으로 추출
fn extract_category_from_wardrobe(query: &str, clothes: &[Clothing]) -> Option<String> {
    let q = query.to_lowercase();

    // 1. sub_category 매칭 (DB 데이터 기반, 하드코딩 없음)
    for c in clothes {
        if let Some(sub) = &c.sub_category {
            let sub_lower = sub.to_lowercase();
            // sub_category를 한국어화해서 비교
            let sub_kr = match sub_lower.as_str() {
                "canvas_sneaker" | "sneaker" => "스니커",
                "slip_on" => "슬립온",
                "trainer" => "트레이너",
                "runner" => "러너",
                "work_boots" => "워크부츠",
                "desert_boots" => "데저트부츠",
                "loafer" => "로퍼",
                "derby" => "더비",
                "chelsea" => "첼시",
                "denim" => "데님",
                "chino" => "치노",
                "cargo" => "카고",
                "slacks" => "슬랙스",
                "tote" => "토트",
                "backpack" => "백팩",
                "crossbody" => "크로스바디",
                "shoulder" => "숄더",
                "helmet" => "헬멧",
                _ => "",
            };
            if !sub_kr.is_empty() && q.contains(sub_kr) {
                return Some(c.category.clone());
            }
        }
        // 2. 아이템 이름의 일부가 쿼리에 포함
        let name_words: Vec<&str> = c.name.split_whitespace().collect();
        let matched_words = name_words.iter().filter(|w| q.contains(&w.to_lowercase())).count();
        if matched_words >= 2 {
            return Some(c.category.clone());
        }
    }

    // 3. 기본 키워드 폴백 (최소한만)
    if q.contains("신발") || q.contains("슈즈") || q.contains("부츠") { return Some("신발".to_string()); }
    if q.contains("아우터") || q.contains("자켓") || q.contains("코트") { return Some("아우터".to_string()); }
    if q.contains("하의") || q.contains("바지") { return Some("하의".to_string()); }
    if q.contains("가방") { return Some("가방".to_string()); }
    if q.contains("상의") { return Some("상의".to_string()); }

    None
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
