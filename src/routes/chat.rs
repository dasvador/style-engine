use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{clothing_repo, feedback_repo};
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::models::feedback::FeedbackRequest;
// 라우트의 DTO(ChatRequest/ImageRequest)와 이름이 겹쳐 alias 한다.
use crate::AppState;
use crate::models::style_vocab::{StyleGenre, Tone, Weight};
use crate::services::llm::{
    ChatRequest as LlmChatRequest, ImageDetail, ImageRequest as LlmImageRequest, LlmClient,
    LlmTask, Message, ToolDef, Usage,
};
use crate::services::outfit_scorer;
use crate::services::weather as weather_service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(chat))
        .route("/image", post(generate_image))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    style_mood: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    material: Option<String>,
}

async fn chat(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    state
        .llm
        .ensure_configured(LlmTask::ChatAgent)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let user_id = &auth.user_id;
    let clothes = if body.gender.is_some() || body.style_mood.is_some() {
        clothing_repo::list_clothing_filtered(
            &state.db,
            body.gender.as_deref(),
            crate::routes::parse_genre(body.style_mood.as_deref())?,
        )
        .await?
    } else {
        clothing_repo::list_clothing(&state.db).await?
    };

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
                    format!("{}°C, {}", w.temperature, w.weather_description)
                }
                Err(_) => String::new(),
            }
        }
        _ => String::new(),
    };

    // 유저 프로파일
    let user_profile = sqlx::query_as::<_, crate::models::user_profile::UserStyleProfile>(
        "SELECT * FROM user_style_profile WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    // 피드백
    let feedback_ctx = {
        let item_scores = feedback_repo::get_item_adjustments(&state.db, user_id)
            .await
            .unwrap_or_default();
        let pref_scores = feedback_repo::get_preference_scores(&state.db, user_id)
            .await
            .unwrap_or_default();
        outfit_scorer::FeedbackContext {
            item_adj: item_scores
                .into_iter()
                .map(|s| (s.item_name, s.score_adjustment))
                .collect(),
            preference: pref_scores
                .into_iter()
                .map(|s| (s.reason_tag, s.score))
                .collect(),
        }
    };

    // ─── Tool definitions ───
    // provider 중립 형태. OpenAI의 `function.parameters`든 Anthropic의 `input_schema`든
    // 직렬화는 provider 구현체가 한다.
    let tools = vec![
        ToolDef::new(
            "search_wardrobe",
            "유저 옷장에서 아이템을 자연어로 검색한다. 유저가 말한 아이템이 어떤 카테고리인지 판단해서 category를 함께 넘겨라.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "검색할 아이템 설명 (예: 올리브 슬립온, 모카브라운 워크자켓)" },
                    "category": { "type": "string", "enum": ["상의","하의","아우터","신발","가방"], "description": "아이템의 카테고리. 슬립온/스니커/부츠/샌들→신발, 자켓/코트/가디건→아우터, 팬츠/데님→하의 등" }
                },
                "required": ["query", "category"]
            }),
        ),
        ToolDef::new(
            "get_outfit",
            "anchor 아이템 기준으로 서버가 최적의 착장을 생성한다. user_query는 유저 원문, anchor_name은 search_wardrobe에서 찾은 정확한 DB 이름.",
            json!({
                "type": "object",
                "properties": {
                    "user_query": { "type": "string", "description": "유저가 언급한 원래 아이템 표현 (예: 올리브 슬립온)" },
                    "anchor_name": { "type": "string", "description": "search_wardrobe 결과에서 선택한 정확한 DB 이름" },
                    "avoid_tags": { "type": "array", "items": { "type": "string" }, "description": "피할 스타일 태그" }
                },
                "required": ["user_query", "anchor_name"]
            }),
        ),
        ToolDef::new(
            "evaluate_outfit",
            "서버가 착장 조합의 품질을 검증한다. 문제가 있으면 이유와 함께 실패를 반환한다. get_outfit 결과를 검증할 때 사용.",
            json!({
                "type": "object",
                "properties": {
                    "item_names": { "type": "array", "items": { "type": "string" }, "description": "검증할 아이템 이름 목록" }
                },
                "required": ["item_names"]
            }),
        ),
        ToolDef::new(
            "submit_feedback",
            "유저가 대화 중 표현한 선호/비선호를 저장한다.",
            json!({
                "type": "object",
                "properties": {
                    "feedback_type": { "type": "string", "enum": ["like", "dislike"], "description": "좋아요/싫어요" },
                    "reason_tags": { "type": "array", "items": { "type": "string" }, "description": "이유 태그 (too_military, good_texture 등)" },
                    "comment": { "type": "string", "description": "유저 원문 피드백" }
                },
                "required": ["feedback_type"]
            }),
        ),
    ];

    let system_prompt = format!(
        r#"너는 프리미엄 셀렉샵 에디토리얼 스타일리스트다. AURALEE, BEAMS, HAVEN 같은 감성으로 코디를 설명한다.

역할:
- 유저의 질문을 이해하고, 도구를 호출해서 답변한다.
- 코디 추천은 반드시 get_outfit 도구를 통해 서버가 결정한다. 직접 아이템을 고르지 마라.
- anchor 아이템을 찾을 때는 search_wardrobe를 호출한다.
- 유저가 싫다/좋다 등 피드백을 주면 submit_feedback을 호출한다.

흐름:
1. 유저가 아이템을 언급하면 → search_wardrobe로 anchor 찾기
2. anchor가 확정되면 → get_outfit으로 서버 추천 받기
3. get_outfit 결과를 evaluate_outfit으로 검증
4. 검증 통과 → 착장을 설명
5. 검증 실패 → get_outfit을 avoid_tags와 함께 재호출
6. 유저가 피드백 주면 → submit_feedback 후 get_outfit 재호출

착장 설명 규칙 (중요):
- 아이템 리스트를 나열하지 마라 (UI에서 이미 표시됨)
- '색상 조화가 좋습니다' 같은 generic 표현 금지
- texture(질감), silhouette(실루엣), visual weight(시각적 무게), grounding(하체 안정감) 중심으로 2~3문장 작성
- 마크다운/볼드/리스트 없이 순수 텍스트로 답변
- 예시: '블루종의 드라이한 면 질감이 상체를 부드럽게 정리하고, 린넨 셔츠가 레이어링에 가벼운 깊이를 만듭니다. 캔버스 스니커로 하체 대비를 잡았습니다.'

날씨: {weather}
답변은 한국어로."#,
        weather = if weather_hint.is_empty() {
            "정보 없음".to_string()
        } else {
            weather_hint.clone()
        },
    );

    // ─── Tool calling loop (최대 8회 반복) ───
    let mut messages: Vec<Message> = vec![Message::user_text(body.message)];
    let mut final_items: Vec<ChatItem> = Vec::new();
    let mut final_reply = String::new();
    let mut first_search_query: Option<String> = None; // 유저 최초 검색어 (덮어쓰기 불가)
    let mut anchor_category: Option<String> = None;

    for _turn in 0..8 {
        let resp = state
            .llm
            .chat(
                LlmTask::ChatAgent,
                LlmChatRequest::new(messages.clone())
                    .system(&system_prompt)
                    .tools(tools.clone()),
            )
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        // 응답 메시지를 히스토리에 추가
        messages.push(Message::Assistant {
            text: resp.text.clone(),
            tool_calls: resp.tool_calls.clone(),
        });

        if !resp.tool_calls.is_empty() {
            for tc in &resp.tool_calls {
                let fn_name = tc.name.as_str();
                let fn_args = &tc.arguments;
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
                        let result =
                            tool_search_wardrobe(query, category, &clothes, &state.embedding).await;
                        tracing::info!(
                            "search_wardrobe(cat={:?}): {}",
                            category,
                            result
                                .char_indices()
                                .nth(300)
                                .map_or(&result[..], |(i, _)| &result[..i])
                        );
                        result
                    }
                    "get_outfit" => {
                        let user_query = fn_args["user_query"]
                            .as_str()
                            .unwrap_or(fn_args["anchor_name"].as_str().unwrap_or(""));
                        let anchor_name = fn_args["anchor_name"].as_str().unwrap_or(user_query);
                        let (outfit_json, mut items) = tool_get_outfit(
                            user_query,
                            anchor_name,
                            &clothes,
                            user_profile.as_ref(),
                            temperature,
                            &feedback_ctx,
                            &state.embedding,
                        )
                        .await;
                        // 유저 원문으로 anchor 슬롯 즉시 교체
                        if let Some(ref uq) = first_search_query {
                            let is_in_db = clothes.iter().any(|c| c.name == *uq);
                            if !is_in_db && let Some(ref cat) = anchor_category {
                                let sk = match cat.as_str() {
                                    "신발" => "shoes",
                                    "아우터" => "outer",
                                    "하의" => "bottom",
                                    "가방" => "bag",
                                    "상의" => "inner",
                                    _ => "",
                                };
                                if let Some(item) = items.iter_mut().find(|i| i.slot == sk) {
                                    item.name = uq.clone();
                                    item.owned = false;
                                }
                            }
                        }
                        final_items = items;
                        outfit_json
                    }
                    "evaluate_outfit" => {
                        let names: Vec<String> = fn_args["item_names"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        tool_evaluate_outfit(&names, &clothes, user_profile.as_ref())
                    }
                    "submit_feedback" => {
                        let fb_type = fn_args["feedback_type"].as_str().unwrap_or("dislike");
                        let reasons: Vec<String> = fn_args["reason_tags"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let comment = fn_args["comment"].as_str().map(String::from);

                        let fb_req = FeedbackRequest {
                            feedback_type: fb_type.to_string(),
                            reasons,
                            inner_name: final_items
                                .iter()
                                .find(|i| i.slot == "inner")
                                .map(|i| i.name.clone()),
                            outer_name: final_items
                                .iter()
                                .find(|i| i.slot == "outer")
                                .map(|i| i.name.clone()),
                            bottom_name: final_items
                                .iter()
                                .find(|i| i.slot == "bottom")
                                .map(|i| i.name.clone()),
                            shoes_name: final_items
                                .iter()
                                .find(|i| i.slot == "shoes")
                                .map(|i| i.name.clone()),
                            bag_name: final_items
                                .iter()
                                .find(|i| i.slot == "bag")
                                .map(|i| i.name.clone()),
                            anchor_name: None,
                            comment,
                        };
                        let _ = feedback_repo::insert_feedback(&state.db, user_id, &fb_req).await;
                        json!({"status": "saved"}).to_string()
                    }
                    _ => json!({"error": "unknown tool"}).to_string(),
                };

                messages.push(Message::ToolResult {
                    id: tc.id.clone(),
                    content: result,
                });
            }
        } else {
            // 도구 호출이 없으면 최종 응답
            final_reply = resp.text_or_empty().to_string();

            // 유저 원문이 DB에 없으면 anchor 슬롯을 원문으로 교체
            if let Some(ref uq) = first_search_query {
                let is_in_db = clothes.iter().any(|c| c.name == *uq);
                tracing::info!(
                    "anchor override check: query='{}' in_db={} cat={:?} items={}",
                    uq,
                    is_in_db,
                    anchor_category,
                    final_items.len()
                );
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
                            tracing::info!(
                                "anchor override: {} '{}' → '{}'",
                                slot_key,
                                item.name,
                                uq
                            );
                            item.name = uq.clone();
                            item.owned = false;
                        } else {
                            tracing::warn!(
                                "anchor override: slot '{}' not found in final_items",
                                slot_key
                            );
                        }
                    }
                }
            }

            break;
        }
    }

    // ─── final_reply가 비어있으면 추가 LLM 호출로 설명 생성 ───
    if final_reply.is_empty() && !final_items.is_empty() {
        tracing::warn!("final_reply empty after tool loop — requesting style note");

        let items_desc = final_items
            .iter()
            .map(|i| format!("{}: {}", i.slot, i.name))
            .collect::<Vec<_>>()
            .join(", ");

        let note_system = "너는 프리미엄 셀렉샵 룩북을 쓰는 에디토리얼 스타일리스트다. 주어진 착장의 Style Note를 2~3문장으로 작성해라.\n\n규칙:\n- 색상 나열이나 '조화가 좋습니다' 같은 generic 표현 금지\n- texture(질감), silhouette(실루엣), visual weight(시각적 무게), grounding(하체 안정감) 중심으로 설명\n- 예시: '블루종의 드라이한 면 질감이 상체를 부드럽게 정리하고, 린넨 셔츠가 레이어링에 가벼운 깊이를 만듭니다. 캔버스 스니커로 하체 대비를 잡고, 워시드 토트가 muted palette에 자연스러운 무게를 추가했습니다.'\n- 마크다운/볼드/리스트 없이 순수 텍스트로";

        let note_user = format!(
            "착장: {}\n날씨: {}\n\nStyle Note:",
            items_desc,
            if weather_hint.is_empty() {
                "정보 없음"
            } else {
                &weather_hint
            }
        );

        match state
            .llm
            .chat(
                LlmTask::StyleNote,
                LlmChatRequest::new(vec![Message::user_text(note_user)]).system(note_system),
            )
            .await
        {
            Ok(resp) => final_reply = resp.text_or_empty().to_string(),
            Err(e) => tracing::warn!("style note fallback failed: {e}"),
        }

        // 그래도 비어있으면 서버 사이드 기본 설명
        if final_reply.is_empty() {
            final_reply = generate_fallback_note(&final_items);
        }
    }

    Ok(Json(ChatResponse {
        reply: final_reply,
        items: final_items,
    }))
}

// ─── Fallback style note (LLM 실패 시) ───
fn generate_fallback_note(items: &[ChatItem]) -> String {
    let outer = items
        .iter()
        .find(|i| i.slot == "outer")
        .map(|i| i.name.as_str());
    let inner = items
        .iter()
        .find(|i| i.slot == "inner")
        .map(|i| i.name.as_str());
    let bottom = items
        .iter()
        .find(|i| i.slot == "bottom")
        .map(|i| i.name.as_str());
    let shoes = items
        .iter()
        .find(|i| i.slot == "shoes")
        .map(|i| i.name.as_str());

    let mut note = String::new();
    if let Some(o) = outer {
        note.push_str(&format!("{}의 질감이 상체 실루엣을 잡아주고, ", o));
    }
    if let Some(i) = inner {
        note.push_str(&format!("{}가 이너 레이어에 가벼운 깊이를 더합니다. ", i));
    }
    if let Some(b) = bottom {
        note.push_str(&format!("{}로 하체 무게감을 안정시키고, ", b));
    }
    if let Some(s) = shoes {
        note.push_str(&format!("{}가 전체 grounding을 완성합니다.", s));
    }
    if note.is_empty() {
        "muted tone의 레이어드 밸런스를 잡은 착장입니다.".to_string()
    } else {
        note
    }
}

// ─── Tool implementations ───

// ─── AI 이미지 생성 ───

#[derive(Debug, Deserialize)]
struct ImageRequest {
    items: String,
    #[serde(default)]
    mood: Option<String>,
}

#[derive(Debug, Serialize)]
struct ImageResponse {
    image_url: Option<String>,
}

async fn generate_image(
    State(state): State<AppState>,
    Json(body): Json<ImageRequest>,
) -> Result<Json<ImageResponse>, AppError> {
    // 캐시 체크: outfit_hash + prompt_hash
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut outfit_hasher = DefaultHasher::new();
    body.items.hash(&mut outfit_hasher);
    let outfit_hash_val = outfit_hasher.finish();
    let outfit_hash = format!("{:016x}", outfit_hash_val);

    // 이미지 프롬프트는 장르마다 다르다. 모르는 값이면 400 으로 돌려준다 —
    // 엉뚱한 장르의 이미지를 만들어 캐시에 넣는 것보다 낫다.
    let genre = crate::routes::parse_genre(body.mood.as_deref())?.unwrap_or(StyleGenre::Amekaji);
    let prompt = build_image_prompt(genre, &body.items, outfit_hash_val);

    let mut prompt_hasher = DefaultHasher::new();
    prompt.hash(&mut prompt_hasher);
    let prompt_hash = format!("{:016x}", prompt_hasher.finish());

    // DB 캐시 확인.
    //
    // 캐시에는 두 종류의 결과가 들어 있다. 통과한 이미지(image_path 있음)와,
    // 통과하지 못했다는 사실(image_path 없음)이다. 후자를 남기지 않으면 검증을
    // 통과하지 못하는 착장이 요청마다 이미지를 새로 생성한다.
    let cached: Option<(Option<String>, i32)> = sqlx::query_as(
        "SELECT image_path, generation_attempts FROM outfit_image \
         WHERE outfit_hash = ? AND prompt_hash = ? LIMIT 1",
    )
    .bind(&outfit_hash)
    .bind(&prompt_hash)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let mut prior_attempts: u32 = 0;
    if let Some((path, attempts)) = cached {
        if let Some(path) = path {
            tracing::info!("image cache hit: {}", path);
            // 적중 횟수는 이 기능의 실제 비용을 좌우한다 — 장당 단가는 미스일 때 값이다.
            // 실패해도 응답을 막을 이유가 없다.
            let _ = sqlx::query(
                "UPDATE outfit_image SET hit_count = hit_count + 1 \
                 WHERE outfit_hash = ? AND prompt_hash = ?",
            )
            .bind(&outfit_hash)
            .bind(&prompt_hash)
            .execute(&state.db)
            .await;
            return Ok(Json(ImageResponse {
                image_url: Some(path),
            }));
        }
        // 통과하지 못한 이력이 있다. 몇 번이나 시도했는지 보고 더 태울지 정한다.
        prior_attempts = attempts.max(0) as u32;
        if prior_attempts >= MAX_TOTAL_ATTEMPTS {
            tracing::info!(
                outfit_hash = %outfit_hash,
                attempts = prior_attempts,
                "검증을 통과하지 못한 착장 — 재생성하지 않고 이미지 없이 응답한다"
            );
            return Ok(Json(ImageResponse { image_url: None }));
        }
    }

    // 예산 확인. 캐시 적중은 돈이 들지 않으므로 위에서 이미 빠져나갔다 —
    // 예산이 소진돼도 이미 만든 이미지는 계속 보인다.
    let daily_budget = budget_from_env("IMAGE_DAILY_BUDGET_USD", DEFAULT_DAILY_BUDGET_USD);
    let monthly_budget = budget_from_env("IMAGE_MONTHLY_BUDGET_USD", DEFAULT_MONTHLY_BUDGET_USD);
    let spent = image_spend(&state.db).await;

    if BUDGET_ENFORCED && !budget_allows_image(&spent, daily_budget, monthly_budget) {
        tracing::warn!(
            event = "image_budget_exhausted",
            today_usd = spent.today,
            month_usd = spent.this_month,
            daily_budget_usd = daily_budget,
            monthly_budget_usd = monthly_budget,
            "이미지 예산 소진 — 생성하지 않고 이미지 없이 응답한다"
        );
        let _ = sqlx::query(
            "INSERT INTO image_daily_spend (spend_date, blocked_requests) \
             VALUES (CURRENT_DATE, 1) AS new \
             ON DUPLICATE KEY UPDATE \
               blocked_requests = image_daily_spend.blocked_requests + new.blocked_requests",
        )
        .execute(&state.db)
        .await;
        return Ok(Json(ImageResponse { image_url: None }));
    }

    // 이미지 생성 + 성별 검증.
    //
    // 검증을 통과하지 못한 이미지는 내보내지 않는다. 여성 모델은 이 제품의
    // 요구사항이고, 검증기는 남성을 놓치기보다 여성을 과하게 떨어뜨리는 쪽으로
    // 치우쳐 있다 (측정: 남성 12/12 차단, 여성 오차단 2/40). 그 편향에서는
    // "실패했지만 내보낸다"가 유일하게 보장을 깨는 경로다.
    //
    // 통과하지 못하면 이미지 없이 응답하고, 그 사실을 캐시한다.
    let mut m = ImageGenMetrics::default();
    let mut outcome: Option<(Option<String>, VerificationStatus)> = None;

    for attempt in 1..=MAX_IMAGE_ATTEMPTS {
        let started = std::time::Instant::now();
        let image = state
            .llm
            .generate_image(&LlmImageRequest {
                prompt: prompt.clone(),
                size: "1024x1536".to_string(),
                quality: "low".to_string(),
            })
            .await;
        m.generate_elapsed += started.elapsed();
        m.generate_calls += 1;

        let image = match image {
            Ok(image) => image,
            Err(e) => {
                tracing::warn!("image generation failed (attempt {attempt}): {e}");
                break;
            }
        };
        m.generate_usage.input_tokens += image.usage.input_tokens;
        m.generate_usage.output_tokens += image.usage.output_tokens;

        // b64 → decode → 파일 저장
        let b64 = image.b64_png.as_str();
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("base64 decode error: {e}")))?;
        let filename = format!("{}.png", uuid::Uuid::new_v4());
        let path = format!("static/images/{}", filename);
        std::fs::write(&path, &bytes)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("file write error: {e}")))?;
        let url = format!("/static/images/{}", filename);
        tracing::info!(
            "image saved (attempt {attempt}): {path} ({} bytes)",
            bytes.len()
        );

        // 생성물 자동 검수: 성별 검증
        let started = std::time::Instant::now();
        let (check, usage) = verify_female_model(&state.llm, b64).await;
        m.verify_elapsed += started.elapsed();
        m.verify_calls += 1;
        m.verify_usage.input_tokens += usage.input_tokens;
        m.verify_usage.output_tokens += usage.output_tokens;

        match next_step(check, attempt) {
            NextStep::Use => {
                tracing::info!("gender check passed (attempt {attempt})");
                outcome = Some((Some(url), VerificationStatus::Passed));
                break;
            }
            NextStep::Retry => {
                tracing::warn!("gender check failed (attempt {attempt}) — retrying");
                let _ = std::fs::remove_file(&path);
            }
            NextStep::GiveUp(status) => {
                tracing::warn!(
                    status = status.as_str(),
                    "검증을 통과하지 못했다 — 이미지 없이 응답한다"
                );
                let _ = std::fs::remove_file(&path);
                outcome = Some((None, status));
                break;
            }
        }
    }

    // 생성 자체가 실패한 경우. 기존과 같이 image_url: null 을 돌려주고,
    // 프런트가 이미지 없이 룩북을 그린다.
    let Some((url, status)) = outcome else {
        return Ok(Json(ImageResponse { image_url: None }));
    };

    // 검증기 장애는 이 착장의 문제가 아니다. 캐시에 남기면 검증기가 회복된 뒤에도
    // 이 착장만 계속 막힌다.
    if status != VerificationStatus::CheckError {
        let _ = sqlx::query(
            "INSERT INTO outfit_image \
             (id, outfit_hash, prompt_hash, image_path, prompt_text, mood, verification_status, generation_attempts) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) AS new \
             ON DUPLICATE KEY UPDATE \
               image_path = new.image_path, \
               verification_status = new.verification_status, \
               generation_attempts = outfit_image.generation_attempts + new.generation_attempts",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&outfit_hash)
        .bind(&prompt_hash)
        .bind(&url)
        .bind(
            prompt
                .char_indices()
                .nth(500)
                .map_or(&prompt[..], |(i, _)| &prompt[..i]),
        )
        .bind(genre.as_str())
        .bind(status.as_str())
        .bind(m.generate_calls)
        .execute(&state.db)
        .await;
    }

    // 쓴 돈은 결과와 무관하게 기록한다. 통과하지 못한 시도도 청구된다.
    m.record_spend(&state.db, &state.llm).await;
    m.record(&state.llm, status, genre, prior_attempts);

    Ok(Json(ImageResponse { image_url: url }))
}

/// 오늘과 이번 달의 이미지 지출.
///
/// 조회에 실패하면 0으로 본다. 예산 조회가 안 된다고 기능을 막으면, DB 장애가
/// 이미지 전면 중단으로 번진다. 상한을 넘길 위험보다 그쪽이 크다.
async fn image_spend(db: &sqlx::MySqlPool) -> ImageSpend {
    sqlx::query_as::<_, ImageSpend>(
        "SELECT \
           COALESCE(SUM(CASE WHEN spend_date = CURRENT_DATE THEN cost_usd END), 0) AS today, \
           COALESCE(SUM(CASE WHEN spend_date >= DATE_FORMAT(CURRENT_DATE, '%Y-%m-01') \
                             THEN cost_usd END), 0) AS this_month \
         FROM image_daily_spend",
    )
    .fetch_one(db)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("이미지 지출 조회 실패 — 0으로 간주한다: {e}");
        ImageSpend::default()
    })
}

/// 최종 이미지 1장을 만드는 데 든 것. 생성과 검증을 따로 센다 —
/// 둘을 합치면 "검증이 전체의 얼마인가"에 답할 수 없다.
#[derive(Default)]
struct ImageGenMetrics {
    generate_calls: u32,
    verify_calls: u32,
    generate_usage: Usage,
    verify_usage: Usage,
    generate_elapsed: std::time::Duration,
    verify_elapsed: std::time::Duration,
}

impl ImageGenMetrics {
    /// 이번 요청에 쓴 추정 비용. 단가표에 없는 모델이면 `None`.
    fn cost_usd(&self, llm: &LlmClient) -> Option<f64> {
        use crate::services::llm::usage::estimate_cost_usd;
        let g = estimate_cost_usd(
            &llm.config().task(LlmTask::ImageGeneration).model,
            &self.generate_usage,
        );
        let v = estimate_cost_usd(
            &llm.config().task(LlmTask::GenderVerify).model,
            &self.verify_usage,
        );
        match (g, v) {
            (Some(g), Some(v)) => Some(g + v),
            _ => None,
        }
    }

    /// 원장에 더한다. 예산은 이 값을 보고 판단한다.
    async fn record_spend(&self, db: &sqlx::MySqlPool, llm: &LlmClient) {
        // 비용을 계산할 수 없는 모델이면 원장이 실제보다 낮아진다. 0으로 채우면
        // 상한이 조용히 무력화되므로, 남기지 않고 경고한다.
        let Some(cost) = self.cost_usd(llm) else {
            tracing::warn!("이미지 비용을 추정할 수 없어 예산 원장에 반영하지 못했다");
            return;
        };
        let _ = sqlx::query(
            "INSERT INTO image_daily_spend (spend_date, cost_usd, generate_calls, verify_calls) \
             VALUES (CURRENT_DATE, ?, ?, ?) AS new \
             ON DUPLICATE KEY UPDATE \
               cost_usd = image_daily_spend.cost_usd + new.cost_usd, \
               generate_calls = image_daily_spend.generate_calls + new.generate_calls, \
               verify_calls = image_daily_spend.verify_calls + new.verify_calls",
        )
        .bind(cost)
        .bind(self.generate_calls)
        .bind(self.verify_calls)
        .execute(db)
        .await;
    }

    /// 이미지 1장 단위의 구조화 로그. `outfit_image` 이벤트만 긁으면
    /// 실패율·평균 시도 횟수·검증이 차지하는 비용과 지연이 나온다.
    fn record(
        &self,
        llm: &LlmClient,
        status: VerificationStatus,
        genre: StyleGenre,
        prior_attempts: u32,
    ) {
        use crate::services::llm::usage::estimate_cost_usd;

        let image_model = &llm.config().task(LlmTask::ImageGeneration).model;
        let verify_model = &llm.config().task(LlmTask::GenderVerify).model;
        let generate_cost = estimate_cost_usd(image_model, &self.generate_usage);
        let verify_cost = estimate_cost_usd(verify_model, &self.verify_usage);

        tracing::info!(
            event = "outfit_image",
            genre = genre.as_str(),
            verification_status = status.as_str(),
            generate_calls = self.generate_calls,
            verify_calls = self.verify_calls,
            // 이전 요청들에서 이미 태운 횟수. 이 착장이 반복해서 실패하는지 보인다.
            prior_attempts = prior_attempts,
            image_model = %image_model,
            verify_model = %verify_model,
            generate_cost_usd = generate_cost,
            verify_cost_usd = verify_cost,
            total_cost_usd = match (generate_cost, verify_cost) {
                (Some(g), Some(v)) => Some(g + v),
                _ => None,
            },
            generate_ms = self.generate_elapsed.as_millis() as u64,
            verify_ms = self.verify_elapsed.as_millis() as u64,
            "outfit image complete"
        );
    }
}

// ─── 무드별 이미지 프롬프트 생성 ───
//
// 룩북 이미지는 의류의 등록 성별과 관계없이 여성 모델을 사용한다.
//
// 남성복 장르도 여성 모델의 스타일링으로 재해석하는 것이 이 기능의 의도다.
// 생성 후 성별 검증(`GenderCheckResult`) 역시 이 콘셉트를 유지하기 위해
// 여성 모델 여부를 확인한다.
//
// `gender` 는 의류 검색 범위를 결정하며, 생성 이미지 모델의 성별을 의미하지
// 않는다. 따라서 이미지 요청(`ImageRequest`)과 캐시 키(`prompt_hash`)에는
// 모델 성별을 별도로 포함하지 않는다.
//
// 그래서 아래 arm 들은 장르마다 옷·소재·상황만 갈라 두고, 인물 묘사는
// `base_face` / `base_body` 로 공유한다.
/// 장르별 촬영 장소 후보와 빛의 성격.
///
/// 프롬프트에 "매번 다른 곳을 고르라" 고 적어도 이미지 모델은 이전 생성 결과를
/// 모르므로 다양성이 보장되지 않는다. 그래서 장소를 코드에서 고른다.
///
/// 고르는 기준은 착장 해시다. 같은 조합이면 늘 같은 장소가 나오므로 프롬프트가
/// 안정적이고, 따라서 `prompt_hash` 도 안정적이라 캐시가 그대로 재사용된다.
/// 조합이 바뀌면 장소도 바뀐다.
///
/// 배경에는 재료나 건축 요소를 하나만 둔다 — 옷보다 시선을 끌면 안 된다.
fn genre_setting(genre: StyleGenre) -> (&'static [&'static str], &'static str) {
    use StyleGenre as G;
    match genre {
        G::MinimalClassic => (
            &[
                "a pale limestone wall with one dark window frame",
                "the smooth concrete side of a modern building, one shadow line across it",
                "a quiet residential street with a plain rendered wall and a single doorway",
            ],
            "soft overcast light from one side",
        ),
        G::RomanticFeminine => (
            &[
                "the ivory-painted exterior of a small cafe, green foliage at one edge",
                "a garden entrance with an old iron gate and planting behind it",
                "a pale stucco wall with a climbing plant running up one side",
            ],
            "gentle warm light from the side, reaching only part of the background",
        ),
        G::ModernChic => (
            &[
                "the dark glass and brushed metal of an office entrance",
                "a polished stone facade with one strong vertical joint",
                "a covered walkway with clean columns receding behind her",
            ],
            "cool daylight raking from the side",
        ),
        G::Bohemian => (
            &[
                "a weathered terracotta plaster wall with dried plants nearby",
                "an old timber door in a sun-warmed stone wall",
                "a shaded courtyard with worn tiles and a large potted plant",
            ],
            "low late-afternoon sun from the side, long soft shadows",
        ),
        G::SportyCasual => (
            &[
                "the edge of a park path where green meets asphalt",
                "a running track fence with the field soft behind it",
                "concrete steps beside a river walkway",
            ],
            "bright open daylight",
        ),
        G::ModelOffDuty => (
            &[
                "a crossing with worn white markings and a stone kerb",
                "a shopfront with a plain awning and a metal handrail",
                "the brick side of an apartment block with one recessed doorway",
                "a bus stop shelter with glass and painted steel",
            ],
            "neutral daylight from the side",
        ),
        G::Street => (
            &[
                "rough concrete and a metal roller shutter in a back alley, a few small weathered posters",
                "a graffitied service door set into a plain concrete wall",
                "a loading bay with corrugated metal and painted floor markings",
            ],
            "hard side light that rakes across the texture and gives the frame depth",
        ),
        G::Mannish => (
            &[
                "old brick and a weathered wooden door",
                "a painted steel shutter, half raised, on a quiet street",
                "a plain office doorway with a stone step",
            ],
            "warm afternoon light from the side",
        ),
        G::SmartCasual => (
            &[
                "a tree-lined pavement outside an office, a glass door catching a soft reflection",
                "a stone building entrance with a shallow flight of steps",
                "a covered arcade with plain columns",
            ],
            "soft morning light from the side",
        ),
        G::Preppy => (
            &[
                "a brick campus building with ivy at one edge of the frame",
                "stone steps leading up to a panelled door",
                "an iron railing along a path with lawn behind it",
            ],
            "clear daylight from the side",
        ),
        G::Workwear => (
            &[
                "a garage doorway with corrugated metal and worn concrete",
                "a timber yard fence with stacked boards behind it",
                "a workshop wall of painted breeze block with one steel door",
            ],
            "flat overcast light with enough direction to show the weave of the fabric",
        ),
        G::OutdoorCasual => (
            &[
                "stone steps at a city trailhead with green foliage behind",
                "a park gate with a gravel path running past it",
                "a concrete underpass opening onto trees",
            ],
            "cool overcast light from one side",
        ),
        G::Amekaji => (
            &[
                "a narrow alley with old brick and a parked bicycle",
                "a shopfront with a faded painted sign and a tiled step",
                "a low wall of weathered concrete with a utility pole beside it",
            ],
            "warm afternoon light from the side",
        ),
    }
}

fn build_image_prompt(genre: StyleGenre, items: &str, hash: u64) -> String {
    let hairstyles = [
        "messy long waves with curtain bangs, effortless undone texture",
        "chin-length blunt bob, slightly tousled",
        "low loose bun with face-framing strands, relaxed and casual",
        "center-part shoulder-length hair, natural air-dried texture",
    ];
    let hair = hairstyles[(hash as usize) % hairstyles.len()];

    // 장소도 착장 해시로 고른다. 헤어와 같은 나머지를 쓰면 둘이 함께 움직이므로
    // 헤어 개수로 한 번 나눈 뒤 고른다.
    let (settings, light) = genre_setting(genre);
    let setting = settings[(hash as usize / hairstyles.len()) % settings.len()];

    // 얼굴·몸을 치수로 지정하지 않는다.
    //
    // 예전 문구는 "very small head proportions", "8.5-head proportion",
    // "175cm tall figure", "narrow waist", "soft feminine V-line face with smooth
    // rounded jawline", "thin soft eyebrows" 처럼 사진 요청이 아니라 캐릭터 모델링
    // 명세에 가까웠다. 거기에 "naturally attractive", "luminous skin" 같은 표현이
    // 겹치면서 결과가 결점 없는 가상 인물로 수렴했다 — 좌우가 지나치게 대칭인 얼굴,
    // 모공이 없는 도자기 피부, 비현실적으로 작은 머리와 긴 다리, 배경과 선명도가
    // 다른 합성 같은 인물.
    //
    // 그래서 비율을 지정하는 대신 "실제 사람"이라고만 말하고, 판단은 모델에 맡긴다.
    // 다만 주름·모공을 강하게 요구하면 반대로 얼굴을 일부러 거칠게 만들기 때문에
    // subtle / natural / unretouched 수준에서 멈춘다.
    let base_face = format!(
        "a real adult woman with natural facial proportions and subtle asymmetry. \
         Realistic skin with visible pores, fine texture, slight tonal variation, \
         natural under-eye detail, individual flyaway hairs, and minimal everyday makeup. \
         Her expression should feel spontaneous rather than posed. Hairstyle: {}.",
        hair
    );
    let base_body = "realistic adult body proportions, natural head-to-body ratio, \
         believable shoulder width and waist, ordinary anatomical variation, \
         natural posture and weight distribution while standing or walking.";

    // 자세는 장소에 맞춰 장르별로 다르게 둔다.
    //
    // 한때 모든 arm 을 같은 보행 포즈로 통일했더니, 서로 다른 코디를 같은 자리에
    // 세워 찍은 것처럼 보였다. 배경도 회색 보도 하나로 수렴해 옷과 장소 사이의
    // 연결이 끊겼다. 그래서 장소와 동작은 장르로 되돌리고, 아래 규칙만 공통으로 건다.
    let base_pose_rule = "Nothing arranged for the camera — she is not touching her hair \
         or face, and she is not posing for the photographer. \
         Full body visible from head to shoes.";

    // 촬영 방식은 장르와 무관하게 같다. 여기서 한 번 정의해 모든 arm 이 같은 문장을
    // 쓴다 — 장르별로 흩어 두면 한쪽만 고쳐지고 나머지가 남는다.
    //
    // 목표는 "평범한 일상 스냅"이 아니라 "실제로 있을 법한, 감각적으로 촬영한 화보"다.
    // 인물은 편하게 두되 화면 전체에는 의도된 구도와 색의 조화가 있어야 한다.
    let base_photo = "Natural fashion editorial photography. Choose a real location \
         whose colours and materials suit the clothes, and let the texture and light of \
         the background carry the mood of the style quietly. The subject looks relaxed and \
         natural, while the frame as a whole feels deliberately composed, with colours in \
         harmony. Shot on a full-frame camera with a 50mm lens, natural available light, \
         realistic skin texture, subtle sensor grain, imperfect fabric folds. \
         A real person photographed on location, not a digitally created fashion avatar.";

    // 사진에 나오는 옷은 추천된 옷이어야 한다.
    //
    // arm 마다 {items} 뒤에 "a plain tee or crisp shirt, an easy leather or denim jacket"
    // 처럼 옷 종류를 다시 적어 두었는데, 이건 추천 결과와 충돌한다. 옷장에 없는 옷이
    // 그려지거나, 새로 추가한 유틸리티 자켓·후드 집업이 데님 재킷으로 바뀐다.
    // 장르는 아래 "어떻게 입었는가" 로만 전하고, 무엇을 입었는지는 {items} 가 정한다.
    let base_fidelity = "Show exactly the garments listed above — do not add, remove or \
         substitute any piece, and do not invent a jacket, bag or shoes that is not listed. \
         If a kind of garment is not in the list, it is not in the photograph. \
         Keep the fit and the material of each listed garment exactly as described — the \
         styling notes below say how the pieces sit together, not how they are cut.";

    // 인물과 배경이 따로 놀지 않게 하는 조건. 빛의 방향·색온도·그림자가 어긋나면
    // 인물만 따로 렌더링해 합성한 것처럼 보인다.
    let base_light = "The subject and the background share one light: the same direction, \
         the same colour temperature, and shadows that agree with each other. \
         She was photographed standing in this place, not cut out and placed into it.";

    // 회피 목록에서 "ugly face", "big head", "ordinary pedestrian look" 을 뺐다.
    // 특히 "ordinary pedestrian look" 을 금지하면 현실에서 볼 법한 사람의 특징을
    // 모델이 의도적으로 지운다. 대신 실제로 걸러야 할 것 — 3D 렌더, 뷰티 필터,
    // 인위적인 신체 비율 — 을 명시한다.
    let base_avoid = "male model, masculine face, masculine bone structure, \
         3D render, CGI, video game character, fashion game avatar, \
         anime face, doll-like face, porcelain skin, plastic skin, \
         beauty filter, excessive skin smoothing, oversized eyes, \
         impossibly small head, elongated limbs, artificial body proportions, \
         perfect facial symmetry, wax figure, mannequin, \
         distorted face, distorted mouth, awkward lip shape, \
         catalog pose, ecommerce posture, stiff standing, symmetrical front pose, \
         cropped body, cropped legs, tight framing, oversaturated colors, harsh lighting, \
         hair clips, jewellery, props or accessories that are not part of the listed outfit, \
         heavy colour grading, a single colour washed over the whole frame, \
         studio backdrop, obvious set dressing, subject lit differently from the background";

    match genre {
        StyleGenre::MinimalClassic => format!(
            r#"Fashion editorial photograph of a woman in her late 20s, dressed in a quiet, understated way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with restraint: clean lines, nothing fussy, no visible branding. Quality reads through texture and drape rather than through logos. {base_fidelity}

Pose: standing still, adjusting a coat cuff, looking away from the camera. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, logos, bold patterns, streetwear elements, sporty pieces, romantic frills, oversaturated colors."#
        ),
        StyleGenre::RomanticFeminine => format!(
            r#"Fashion editorial photograph of a woman in her early 20s, dressed in a soft, romantic way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with intentional softness — the lighter, more fluid pieces allowed to move, the proportions considered rather than sweet. Romantic and grown-up, never childish. {base_fidelity}

Pose: standing near a doorway, one hand resting on her bag strap, weight on one leg. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, masculine styling, dark heavy tones, oversized baggy fit, street edge, sporty elements."#
        ),
        StyleGenre::ModernChic => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s, dressed in a sharp, tailored way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn sharply: a controlled silhouette, precise lines, everything sitting exactly where it should. Pressed and intentional, closer to soft office than to costume. {base_fidelity}

Pose: mid-stride, walking a few steps, looking ahead. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, casual sneakers, oversized baggy fit, vintage distressing, romantic frills, sporty elements."#
        ),
        StyleGenre::Bohemian => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in a loose, bohemian way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn loosely: layers that move, texture carrying the look, warm and lived-in. Polished rather than hippie. {base_fidelity}

Pose: standing with one hand in a pocket, hair moving slightly in the air. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, minimal clean styling, corporate look, sporty elements, neon colors, tech fabrics."#
        ),
        // 이 프롬프트의 내용(레깅스·스포츠브라·바이커숏·애슬레저)은 원래부터
        // 스포티 캐주얼이었다. 예전 분류가 "모델 사복 + 애슬레저"를 한 장르로
        // 묶고 있어서 off_duty 에 붙어 있었을 뿐이라, 문구만 장르에 맞춘다.
        StyleGenre::SportyCasual => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in sportswear worn as daily clothes. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn for movement: what is fitted stays close to the body, what is relaxed is allowed to hang. Comfortable but deliberate — someone running errands after a workout, not on the way to the gym. {base_fidelity}

Pose: mid-stride on a path, bag on one shoulder, relaxed and unhurried. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, formal styling, vintage distressing, dark moody tones, heavy makeup, aggressive gym energy, harsh lighting."#
        ),
        // 신규 장르. 애슬레저를 스포티 캐주얼로 분리하면서 비게 된 자리를 채운다.
        // 데님·가죽재킷·티셔츠 같은 기본 아이템에 힘을 뺀 구성이 이 장르의 정의다.
        StyleGenre::ModelOffDuty => format!(
            r#"Fashion editorial photograph of a woman in her 20s on a normal day out. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with deliberate proportion: one volume played against another — something close to the body against something relaxed — and the materials left to contrast, a soft knit or jersey against rigid denim or smooth leather. The pieces are simple, but the fit and the balance are chosen rather than accidental. {base_fidelity}

Pose: walking at an easy pace, looking off to the side. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, runway styling, evening wear, heavy layering, athletic leggings, sports bra, gym clothing, romantic frills."#
        ),
        StyleGenre::Street => format!(
            r#"Fashion editorial photograph of a woman in her late teens or early 20s, dressed in streetwear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with intentional imbalance: volume set against fit, layers that clash on purpose. Confident rather than tidy, never accidental. {base_fidelity}

Pose: leaning one shoulder against the wall, hands in pockets, chin slightly down. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, feminine soft styling, luxury campaign mood, romantic atmosphere, pastel tones."#
        ),
        StyleGenre::Mannish => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in menswear-leaning pieces. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn loosely and squarely, as if borrowed rather than fitted. Fabrics show soft fading and wear. {base_fidelity}

Pose: standing squarely, hands in trouser pockets, looking straight down the street. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, feminine delicate styling, formal look, luxury campaign mood."#
        ),
        StyleGenre::SmartCasual => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s on her way to work. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn neatly but not stiffly — pressed, considered proportions, one step down from a suit and one step up from casual. No visible branding. {base_fidelity}

Pose: walking with a bag in one hand, caught mid-step. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, gym clothing, distressed vintage, heavy streetwear, evening glamour, romantic frills."#
        ),
        StyleGenre::Preppy => format!(
            r#"Fashion editorial photograph of a woman in her early 20s, dressed in preppy pieces. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn tidily: neat collars, layers sitting flat, classic proportions. Collegiate and upbeat rather than formal. {base_fidelity}

Pose: standing on stone steps, one hand resting on the railing. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, streetwear, athletic gear, distressed denim, evening wear, bohemian layering."#
        ),
        StyleGenre::Workwear => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s, dressed in workwear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn like clothes that get used: sleeves pushed back, honest fading at the seams and cuffs, hardware that has seen work. Never decorative distressing. {base_fidelity}

Pose: standing with sleeves pushed to the forearm, hands in jacket pockets. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, delicate fabrics, tailored formalwear, athletic gear, romantic frills, glossy luxury styling."#
        ),
        StyleGenre::OutdoorCasual => format!(
            r#"Fashion editorial photograph of a woman in her 20s in the city, dressed in outdoor gear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn as real equipment rather than as a look: functional details visible, drawcords and straps adjusted, layered for weather. Gear that has been outdoors, not a gym outfit. {base_fidelity}

Pose: adjusting a pack strap on one shoulder, caught mid-step. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, leggings, sports bra, gym styling, tailored formalwear, romantic frills, luxury campaign gloss."#
        ),
        StyleGenre::Amekaji => format!(
            // 남성/공용 기본 — 기존 힙스터 스타일
            r#"Fashion editorial photograph of a woman in her 20s, dressed in relaxed American casual. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn in rather than new: soft washes, natural fading, a relaxed silhouette. Nothing pristine. {base_fidelity}

Pose: standing relaxed, one hand in a pocket, looking down the alley. {base_pose_rule}

Setting: {setting}; {light}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. {base_light}

Aesthetic: {base_photo}

Avoid: {base_avoid}, tight-fitting clothes, formal styling, luxury campaign mood."#
        ),
    }
}

// ─── 성별 검증 (GPT-4o-mini vision) ───

// ─── 이미지 생성 예산 ───
//
// 이 앱에서 돈을 쓰는 곳은 사실상 이미지 하나다 (실측: 이미지 $0.0123/장,
// 채팅 $0.0023/턴). 그래서 이미지만 막아도 총액이 잡히고, 채팅과 추천은
// 예산과 무관하게 계속 동작한다 — 채팅으로 $10 을 쓰려면 하루 4,300턴이 필요하다.

/// 상한을 실제로 적용할지.
///
/// **지금은 꺼져 있다.** 실사용 데이터가 아직 얼마 없어서 상한이 개발 중에만
/// 걸리기 때문이다. 지출 기록(`image_daily_spend`)은 계속 쌓이므로, 며칠 뒤
/// 실제 사용량을 보고 상한값을 정한 다음 `true` 로 되돌리면 된다.
///
/// 코드를 주석 처리하지 않고 플래그로 둔 이유: 주석 처리한 코드는 컴파일되지
/// 않아 조용히 낡는다. 이 상태에서도 예산 로직은 계속 타입 검사와 테스트를 받는다.
const BUDGET_ENFORCED: bool = false;

/// 한 달 상한. 이게 실제 보장이다. 기본 $10.
const DEFAULT_MONTHLY_BUDGET_USD: f64 = 10.0;

/// 하루 상한. 한 달치를 하루에 태우지 못하게 하는 방지선이다.
/// 월 상한의 1/10 이라 정상 사용에서는 걸리지 않는다.
const DEFAULT_DAILY_BUDGET_USD: f64 = 1.0;

/// 이미지 1장(생성 1회 + 검증 1회)의 실측 비용.
/// 생성 전에는 토큰 수를 모르므로, 예산을 넘길지 판단할 때 이 값을 쓴다.
const IMAGE_COST_ESTIMATE_USD: f64 = 0.0123;

fn budget_from_env(var: &str, default: f64) -> f64 {
    match std::env::var(var) {
        Ok(raw) => match raw.parse::<f64>() {
            Ok(v) if v >= 0.0 => v,
            _ => {
                tracing::warn!("{var} 값을 해석할 수 없어 기본값 {default} 을 사용합니다: {raw}");
                default
            }
        },
        Err(_) => default,
    }
}

/// 이번 요청의 이미지를 만들어도 되는가.
///
/// 다 쓴 뒤에 막는 것이 아니라 **한 장 값을 더 써도 상한을 넘지 않을 때만** 허용한다.
/// 그래야 상한이 실제 상한이 된다.
fn budget_allows_image(spent: &ImageSpend, daily: f64, monthly: f64) -> bool {
    spent.today + IMAGE_COST_ESTIMATE_USD <= daily
        && spent.this_month + IMAGE_COST_ESTIMATE_USD <= monthly
}

#[derive(Debug, Clone, Copy, Default, sqlx::FromRow)]
struct ImageSpend {
    today: f64,
    this_month: f64,
}

/// 한 요청에서 시도할 최대 생성 횟수.
const MAX_IMAGE_ATTEMPTS: u32 = 3;

/// 한 착장에 대해 누적으로 허용하는 생성 횟수.
///
/// 검증기의 오판(측정 5%)은 대체로 무작위라 다시 생성하면 대개 통과한다. 하지만
/// 어떤 착장은 구조적으로 계속 떨어질 수 있고, 그런 착장에 매 요청 3장씩 무한히
/// 태울 수는 없다. 여기까지 쓰면 그 착장은 이미지 없이 응답한다.
const MAX_TOTAL_ATTEMPTS: u32 = 9;

/// 누적 상한이 한 요청분보다 작으면 첫 요청부터 상한에 걸려 재시도가 아예 일어나지 않는다.
const _: () = assert!(MAX_TOTAL_ATTEMPTS > MAX_IMAGE_ATTEMPTS);

/// 성별 검증 한 번의 결과.
///
/// "여성이 아니다"와 "검증기를 부를 수 없었다"는 다른 사건이다. 둘을 bool 하나로
/// 뭉개면 재생성해야 할 상황과 그래봐야 소용없는 상황이 구분되지 않고,
/// 나중에 실제 실패율을 물었을 때도 답할 수 없다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenderCheckResult {
    Female,
    NotFemale,
    /// 검증 API 오류. 생성 파이프라인을 막지는 않는다.
    Unavailable,
}

/// 반환한 이미지가 어떤 경위로 결정됐는지. `outfit_image.verification_status`
/// ENUM 과 값이 일치해야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerificationStatus {
    Passed,
    /// 재시도 후에도 통과하지 못했다. 이미지를 내보내지 않는다.
    Rejected,
    /// 검증기를 부를 수 없었다. 검증하지 못한 이미지도 내보내지 않는다.
    CheckError,
}

impl VerificationStatus {
    fn as_str(self) -> &'static str {
        match self {
            VerificationStatus::Passed => "passed",
            VerificationStatus::Rejected => "rejected",
            VerificationStatus::CheckError => "check_error",
        }
    }
}

/// 검증 결과를 받고 무엇을 할지.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextStep {
    /// 이 이미지를 사용한다.
    Use,
    /// 이 이미지를 버리고 다시 생성한다.
    Retry,
    /// 이 이미지를 버리고 중단한다. 이미지 없이 응답한다.
    GiveUp(VerificationStatus),
}

/// 여성 모델 보장이 지켜지는 지점.
///
/// `Use` 는 오직 `Female` 에서만 나온다. 검증에 실패했거나 검증하지 못한 이미지는
/// 어느 경로로도 사용되지 않는다. 네트워크도 DB도 타지 않으므로 테스트로 고정한다.
fn next_step(check: GenderCheckResult, attempt: u32) -> NextStep {
    match check {
        GenderCheckResult::Female => NextStep::Use,
        // 검증기가 죽은 것은 이미지의 문제가 아니라 다시 생성해도 같은 상태다.
        // 재시도하지 않고, 검증하지 못한 이미지는 내보내지 않는다.
        GenderCheckResult::Unavailable => NextStep::GiveUp(VerificationStatus::CheckError),
        GenderCheckResult::NotFemale if attempt < MAX_IMAGE_ATTEMPTS => NextStep::Retry,
        GenderCheckResult::NotFemale => NextStep::GiveUp(VerificationStatus::Rejected),
    }
}

/// 검증 1회. 사용량도 함께 돌려준다 — 호출부가 검증 비용을 따로 집계한다.
async fn verify_female_model(llm: &LlmClient, b64_image: &str) -> (GenderCheckResult, Usage) {
    // 저해상도로 충분할 것 같지만 아니었다. 같은 40장을 두 해상도로 검증해 보면
    // 오답이 Low 2/40 → 9/40 로 늘어난다 (전부 여성을 남성으로 판정하는 방향).
    // 오답 1건은 이미지를 통째로 다시 만들게 하므로, 호출당 아끼는 $0.005 보다
    // 재생성 비용과 12초가 크다. 검증은 자주 부르는 대신 정확해야 한다.
    let req = LlmChatRequest::new(vec![Message::user_image_with_detail(
        "Is the person in this photo female? Reply with only 'yes' or 'no'.",
        format!("data:image/png;base64,{}", b64_image),
        ImageDetail::High,
    )]);

    match llm.chat(LlmTask::GenderVerify, req).await {
        Ok(resp) => {
            let result = if resp.text_or_empty().to_lowercase().contains("yes") {
                GenderCheckResult::Female
            } else {
                GenderCheckResult::NotFemale
            };
            (result, resp.usage)
        }
        Err(e) => {
            // 검수기가 죽었다고 생성 파이프라인을 막지는 않는다. 다만 통과와
            // 같이 기록하지도 않는다 — 그러면 검증이 도는지조차 알 수 없다.
            tracing::warn!("gender verification unavailable: {e}");
            (GenderCheckResult::Unavailable, Usage::default())
        }
    }
}

// ─── Tool implementations ───

async fn tool_search_wardrobe(
    query: &str,
    category: Option<&str>,
    clothes: &[Clothing],
    embedding: &std::sync::Arc<crate::services::embedding::EmbeddingService>,
) -> String {
    // LLM이 판단한 카테고리로 필터 (없으면 전체 검색)
    let search_clothes: Vec<Clothing> = if let Some(cat) = category {
        clothes
            .iter()
            .filter(|c| c.category == cat)
            .cloned()
            .collect()
    } else {
        clothes.to_vec()
    };

    // 임베딩 기반 시맨틱 검색
    match embedding.search_wardrobe(query, &search_clothes, 5).await {
        Ok(matches) => {
            let results: Vec<serde_json::Value> = matches
                .iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "category": m.category,
                        "confidence": (m.similarity * 100.0).round() / 100.0,
                    })
                })
                .collect();
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
                    results
                        .push(json!({"name": c.name, "category": c.category, "confidence": 0.6}));
                }
            }
            results.truncate(5);
            serde_json::to_string(&results).unwrap_or("[]".to_string())
        }
    }
}

async fn tool_get_outfit(
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
    let fuzzy = || {
        clothes
            .iter()
            .filter(|c| cat_hint.is_none_or(|cat| c.category == cat))
            .find(|c| {
                let n = c.name.to_lowercase();
                q.split_whitespace()
                    .filter(|w| *w != "신발" && *w != "색")
                    .all(|w| n.contains(w))
            })
    };

    // scoring용 proxy anchor (정확 → fuzzy → 임베딩)
    let proxy_anchor = match exact.or_else(fuzzy) {
        Some(a) => Some(a),
        None => {
            let filtered: Vec<Clothing> = clothes
                .iter()
                .filter(|c| cat_hint.is_none_or(|cat| c.category == cat))
                .cloned()
                .collect();
            embedding
                .search_wardrobe(anchor_name, &filtered, 1)
                .await
                .ok()
                .and_then(|m| m.into_iter().next())
                .filter(|m| m.similarity > 0.5)
                .and_then(|m| clothes.iter().find(|c| c.name == m.name))
        }
    };
    let anchor_owned = exact.is_some(); // 정확히 DB에 있는 경우만 owned
    let display_anchor_name = user_query.to_string(); // 항상 유저 원문 유지
    let display_anchor_cat = cat_hint.unwrap_or("상의");

    if let Some(pa) = proxy_anchor {
        tracing::info!(
            "get_outfit: proxy anchor='{}' for query='{}'",
            pa.name,
            anchor_name
        );
    } else {
        tracing::info!(
            "get_outfit: no DB match for '{}', using as unowned anchor",
            anchor_name
        );
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
                "상의" => "inner",
                "아우터" => "outer",
                "하의" => "bottom",
                "신발" => "shoes",
                "가방" => "bag",
                _ => "shoes",
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
                    material: None,
                });
            }

            let desc = items
                .iter()
                .map(|i| format!("{}: {}", i.slot, i.name))
                .collect::<Vec<_>>()
                .join("\n");
            let items_json: Vec<serde_json::Value> = items.iter().map(|i| {
                json!({"slot": i.slot, "name": i.name, "category": i.category, "owned": i.owned})
            }).collect();
            let response = json!({ "outfit": desc, "items": items_json });
            (response.to_string(), items)
        }
        None => (
            json!({"error": "no suitable outfit found"}).to_string(),
            Vec::new(),
        ),
    }
}

fn tool_evaluate_outfit(
    names: &[String],
    clothes: &[Clothing],
    _user: Option<&crate::models::user_profile::UserStyleProfile>,
) -> String {
    let items: Vec<&Clothing> = names
        .iter()
        .filter_map(|n| clothes.iter().find(|c| c.name == *n))
        .collect();

    if items.len() < 2 {
        return json!({"pass": false, "issues": ["아이템을 2개 이상 찾을 수 없습니다"]})
            .to_string();
    }

    let mut issues: Vec<String> = Vec::new();

    // military/workwear 과밀
    let strong_count = items
        .iter()
        .filter(|i| i.strong_style_score.unwrap_or(1) >= 5)
        .count();
    if strong_count >= 3 {
        issues.push("too_military: 강한 스타일 아이템이 3개 이상".to_string());
    }

    // 같은 색상군 3+
    let mut cg_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for i in &items {
        let cg = outfit_scorer::color_group(i.color.as_deref().unwrap_or(""));
        if cg != "other" {
            *cg_counts.entry(cg).or_insert(0) += 1;
        }
    }
    for (cg, count) in &cg_counts {
        if *count >= 3 {
            issues.push(format!("color_repetition: {} 색상이 {}개 반복", cg, count));
        }
    }

    // 전부 어두움
    let dark_count = items.iter().filter(|i| i.tone == Some(Tone::Dark)).count();
    if dark_count >= 3 {
        issues.push("too_dark: 어두운 톤이 3개 이상".to_string());
    }

    // floating
    let avg_float: f32 = items
        .iter()
        .filter_map(|i| i.floating_score)
        .map(|f| f as f32)
        .sum::<f32>()
        / items.len().max(1) as f32;
    if avg_float >= 5.0 {
        issues.push("floating_balance: 전체적으로 떠보임".to_string());
    }

    // texture 단조
    let avg_tex: f32 = items
        .iter()
        .filter_map(|i| i.texture_depth_v2)
        .map(|t| t as f32)
        .sum::<f32>()
        / items.len().max(1) as f32;
    if avg_tex < 2.5 {
        issues.push("too_flat: 질감이 너무 밋밋함".to_string());
    }

    // grounding 부족 (신발+가방)
    let grounding: i32 = items
        .iter()
        .filter(|i| i.category == "신발" || i.category == "가방")
        .filter_map(|i| i.grounding_score)
        .map(|g| g as i32)
        .sum();
    if grounding <= 3 {
        issues.push("low_grounding: 접지감 부족".to_string());
    }

    let pass = issues.is_empty();
    json!({"pass": pass, "issues": issues, "score_summary": {
        "strong_style_count": strong_count,
        "dark_tone_count": dark_count,
        "avg_floating": avg_float,
        "avg_texture": avg_tex,
        "grounding": grounding,
    }})
    .to_string()
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
        let mut scored: Vec<(&Clothing, i32)> = clothes
            .iter()
            .filter(|c| c.category == cat && c.id != anchor.id)
            .filter(|c| is_weather_appropriate(c, temp))
            .map(|c| (c, outfit_scorer::complement_score(anchor, c)))
            .collect();
        scored.sort_by_key(|a| std::cmp::Reverse(a.1));

        let mut result = Vec::new();
        let mut sub_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (c, _) in &scored {
            let sub = c.sub_category.as_deref().unwrap_or("other").to_string();
            let count = sub_counts.entry(sub).or_insert(0);
            if *count < 2 {
                // 같은 sub_category 최대 2개
                result.push(*c);
                *count += 1;
            }
            if result.len() >= k {
                break;
            }
        }
        result
    };

    let tops = if anchor_cat == "상의" {
        vec![anchor]
    } else {
        slot_candidates("상의", 5)
    };
    let bottoms = if anchor_cat == "하의" {
        vec![anchor]
    } else {
        slot_candidates("하의", 5)
    };
    let outers_pool = if anchor_cat == "아우터" {
        vec![anchor]
    } else {
        slot_candidates("아우터", 4)
    };
    let shoes = if anchor_cat == "신발" {
        vec![anchor]
    } else {
        slot_candidates("신발", 4)
    };
    let bags = if anchor_cat == "가방" {
        vec![anchor]
    } else {
        slot_candidates("가방", 3)
    };

    let mut combos: Vec<(Vec<&Clothing>, i32)> = Vec::new();

    for top in &tops {
        for bottom in &bottoms {
            for shoe in &shoes {
                for bag in &bags {
                    let outfit = vec![*top, *bottom, *shoe, *bag];
                    let score = outfit_scorer::total_outfit_score_with_feedback(
                        anchor, &outfit, user, feedback,
                    );
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
                        let score = outfit_scorer::total_outfit_score_with_feedback(
                            anchor, &outfit, user, feedback,
                        );
                        combos.push((outfit, score));
                    }
                }
            }
        }
    }

    combos.sort_by_key(|a| std::cmp::Reverse(a.1));
    let (best_outfit, best_score) = combos.first()?;

    tracing::info!(
        "final outfit (score={}): {}",
        best_score,
        best_outfit
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    );

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
        desc_parts.push(format!("{}: {}", slot, c.name));
        let mat = c
            .material_primary
            .clone()
            .or_else(|| c.texture_keywords.clone());
        items.push(ChatItem {
            slot: slot.to_string(),
            category: c.category.clone(),
            name: c.name.clone(),
            owned: true,
            material: mat,
        });
    }
    if !items.iter().any(|i| i.name == anchor.name) {
        let slot = match anchor.category.as_str() {
            "상의" => "inner",
            "아우터" => "outer",
            "하의" => "bottom",
            "신발" => "shoes",
            "가방" => "bag",
            _ => "?",
        };
        desc_parts.push(format!("{}: {}", slot, anchor.name));
        let mat = anchor
            .material_primary
            .clone()
            .or_else(|| anchor.texture_keywords.clone());
        items.push(ChatItem {
            slot: slot.to_string(),
            category: anchor.category.clone(),
            name: anchor.name.clone(),
            owned: clothes.iter().any(|c| c.name == anchor.name),
            material: mat,
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
        let matched_words = name_words
            .iter()
            .filter(|w| q.contains(&w.to_lowercase()))
            .count();
        if matched_words >= 2 {
            return Some(c.category.clone());
        }
    }

    // 3. 기본 키워드 폴백 (최소한만)
    if q.contains("신발") || q.contains("슈즈") || q.contains("부츠") {
        return Some("신발".to_string());
    }
    if q.contains("아우터") || q.contains("자켓") || q.contains("코트") {
        return Some("아우터".to_string());
    }
    if q.contains("하의") || q.contains("바지") {
        return Some("하의".to_string());
    }
    if q.contains("가방") {
        return Some("가방".to_string());
    }
    if q.contains("상의") {
        return Some("상의".to_string());
    }

    None
}

fn is_weather_appropriate(item: &Clothing, temp: f64) -> bool {
    let weight = item.weight.unwrap_or(Weight::Mid);
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;
    if temp >= 20.0 {
        if mat == "wool" || mat == "flannel" {
            return false;
        }
        if name.contains("니트") && !name.contains("가벼") {
            return false;
        }
        if name.contains("울 ") {
            return false;
        }
        if item.category == "아우터" && weight == Weight::Heavy {
            return false;
        }
        if name.contains("코트") || name.contains("파카") {
            return false;
        }
    }
    if temp >= 25.0 {
        if item.category == "아우터" && weight != Weight::Light {
            return false;
        }
        if name.contains("코듀로이") {
            return false;
        }
        if weight == Weight::Heavy {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `verification_status` 는 DB ENUM 이다. Rust 쪽 문자열이 하나라도 어긋나면
    /// 그 상태의 이미지만 INSERT 가 실패하고, 증상은 "특정 경우에만 캐시가 안 걸림"
    /// 으로 나타난다 — 지금 고치고 있는 버그와 똑같은 모양이다. 정의와 대조해 둔다.
    #[test]
    fn verification_status_matches_db_enum() {
        let sql = include_str!("../../migrations/20260910000001_guarantee_verified_images.sql");
        for status in [
            VerificationStatus::Passed,
            VerificationStatus::Rejected,
            VerificationStatus::CheckError,
        ] {
            assert!(
                sql.contains(&format!("'{}'", status.as_str())),
                "{} 가 마이그레이션의 ENUM 정의에 없다",
                status.as_str()
            );
        }
    }

    /// 이 제품의 요구사항: 검증을 통과하지 못한 이미지는 어떤 경로로도 나가지 않는다.
    /// 시도 횟수나 검증기 상태와 무관하게 성립해야 한다.
    #[test]
    fn only_verified_female_images_are_used() {
        for attempt in 1..=MAX_IMAGE_ATTEMPTS + 2 {
            for check in [
                GenderCheckResult::Female,
                GenderCheckResult::NotFemale,
                GenderCheckResult::Unavailable,
            ] {
                let used = next_step(check, attempt) == NextStep::Use;
                assert_eq!(
                    used,
                    check == GenderCheckResult::Female,
                    "check={check:?} attempt={attempt}"
                );
            }
        }
    }

    #[test]
    fn retries_until_the_last_attempt_then_gives_up() {
        for attempt in 1..MAX_IMAGE_ATTEMPTS {
            assert_eq!(
                next_step(GenderCheckResult::NotFemale, attempt),
                NextStep::Retry,
                "attempt={attempt}"
            );
        }
        assert_eq!(
            next_step(GenderCheckResult::NotFemale, MAX_IMAGE_ATTEMPTS),
            NextStep::GiveUp(VerificationStatus::Rejected)
        );
    }

    /// 상한은 "다 쓴 뒤"가 아니라 "한 장 값을 더 써도 넘지 않을 때"를 기준으로 해야
    /// 실제 상한이 된다. 경계에서 한 장이 더 나가면 월 상한을 넘긴다.
    #[test]
    fn budget_stops_before_exceeding_not_after() {
        let (daily, monthly) = (1.0, 10.0);
        let exactly_one_left = ImageSpend {
            today: daily - IMAGE_COST_ESTIMATE_USD,
            this_month: monthly - IMAGE_COST_ESTIMATE_USD,
        };
        assert!(budget_allows_image(&exactly_one_left, daily, monthly));

        let a_cent_short = ImageSpend {
            today: daily - IMAGE_COST_ESTIMATE_USD + 0.0001,
            this_month: 0.0,
        };
        assert!(!budget_allows_image(&a_cent_short, daily, monthly));
    }

    /// 월 상한이 실제 보장이다. 하루치가 남아 있어도 월이 차면 막혀야 한다.
    #[test]
    fn monthly_budget_binds_even_with_daily_room() {
        let spent = ImageSpend {
            today: 0.0,
            this_month: 10.0,
        };
        assert!(!budget_allows_image(&spent, 1.0, 10.0));
    }

    /// 예산을 0 으로 두면 이미지 생성이 완전히 꺼진다.
    #[test]
    fn zero_budget_disables_image_generation() {
        assert!(!budget_allows_image(&ImageSpend::default(), 0.0, 0.0));
    }

    /// 검증기 장애는 재생성으로 나아지지 않는다. 첫 시도에서 바로 중단해야 한다.
    #[test]
    fn verifier_outage_does_not_burn_retries() {
        for attempt in 1..=MAX_IMAGE_ATTEMPTS {
            assert_eq!(
                next_step(GenderCheckResult::Unavailable, attempt),
                NextStep::GiveUp(VerificationStatus::CheckError),
                "attempt={attempt}"
            );
        }
    }
}

#[cfg(test)]
mod image_prompt_tests {
    use super::*;

    /// 같은 착장이면 프롬프트가 그대로여야 한다.
    ///
    /// 장소를 착장 해시로 고르므로, 같은 조합에서 장소가 흔들리면 `prompt_hash` 가
    /// 달라지고 캐시가 매번 빗나간다 — 이미지를 다시 생성하니 비용이 그대로 늘어난다.
    #[test]
    fn same_outfit_gives_the_same_prompt() {
        let a = build_image_prompt(StyleGenre::ModelOffDuty, "회색 티, 데님", 12345);
        let b = build_image_prompt(StyleGenre::ModelOffDuty, "회색 티, 데님", 12345);
        assert_eq!(a, b);
    }

    /// 착장이 다르면 장소도 실제로 달라져야 한다.
    ///
    /// 프롬프트에 "매번 다른 곳을 고르라" 고 적는 것으로는 보장되지 않는다 —
    /// 이미지 모델은 이전 생성 결과를 모른다. 그래서 코드가 고르고, 그 선택이
    /// 정말 갈리는지 여기서 확인한다.
    #[test]
    fn different_outfits_reach_different_settings() {
        let (settings, _) = genre_setting(StyleGenre::ModelOffDuty);
        assert!(settings.len() >= 2, "장소 후보가 하나뿐이면 다양성이 없다");

        let seen: std::collections::HashSet<&str> = (0..200u64)
            .map(|h| {
                let hairstyles = 4usize; // build_image_prompt 과 같은 개수
                settings[(h as usize / hairstyles) % settings.len()]
            })
            .collect();
        assert_eq!(
            seen.len(),
            settings.len(),
            "해시를 훑어도 일부 장소에 도달하지 못한다"
        );
    }

    /// 모든 장르에 장소 후보가 둘 이상 있어야 한다. 하나뿐이면 그 장르는
    /// 코디가 바뀌어도 늘 같은 자리에서 찍힌다.
    #[test]
    fn every_genre_has_more_than_one_setting() {
        for &g in StyleGenre::ALL {
            let (settings, light) = genre_setting(g);
            assert!(
                settings.len() >= 2,
                "{g} 의 장소 후보가 {}개",
                settings.len()
            );
            assert!(!light.is_empty(), "{g} 에 빛 설명이 없다");
        }
    }

    /// 추천된 옷이 프롬프트에 그대로 들어가고, 그대로 그리라는 조건이 붙어야 한다.
    #[test]
    fn prompt_carries_the_items_and_the_fidelity_rule() {
        let p = build_image_prompt(StyleGenre::Preppy, "네이비 블레이저, 치노", 7);
        assert!(p.contains("네이비 블레이저, 치노"));
        assert!(p.contains("do not add, remove or substitute"));
        assert!(p.contains("Keep the fit and the material"));
    }
}
