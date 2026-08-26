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
use crate::models::style_vocab::{Tone, Weight};
use crate::services::llm::{
    ChatRequest as LlmChatRequest, ImageRequest as LlmImageRequest, LlmClient, LlmTask, Message,
    ToolDef,
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
            body.style_mood.as_deref(),
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

    let mood = body.mood.as_deref().unwrap_or("amekaji");
    let prompt = build_image_prompt(mood, &body.items, outfit_hash_val);

    let mut prompt_hasher = DefaultHasher::new();
    prompt.hash(&mut prompt_hasher);
    let prompt_hash = format!("{:016x}", prompt_hasher.finish());

    // DB 캐시 확인
    let cached: Option<String> = sqlx::query_scalar(
        "SELECT image_path FROM outfit_image WHERE outfit_hash = ? AND prompt_hash = ? LIMIT 1",
    )
    .bind(&outfit_hash)
    .bind(&prompt_hash)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    if let Some(path) = cached {
        tracing::info!("image cache hit: {}", path);
        return Ok(Json(ImageResponse {
            image_url: Some(path),
        }));
    }

    // 이미지 생성 + 성별 검증 (최대 3회 시도)
    let mut final_url: Option<String> = None;

    for attempt in 0..3 {
        let image = state
            .llm
            .generate_image(&LlmImageRequest {
                prompt: prompt.clone(),
                size: "1024x1536".to_string(),
                quality: "low".to_string(),
            })
            .await;

        // b64 → decode → 파일 저장
        let image = match image {
            Ok(image) => image,
            Err(e) => {
                tracing::warn!("image generation failed (attempt {}): {e}", attempt + 1);
                break;
            }
        };

        {
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
                "image saved (attempt {}): {} ({} bytes)",
                attempt + 1,
                path,
                bytes.len()
            );

            // 생성물 자동 검수: 성별 검증
            let is_female = verify_female_model(&state.llm, b64).await;
            if is_female {
                tracing::info!("gender check passed (attempt {})", attempt + 1);
                // DB 캐시 저장
                let _ = sqlx::query(
                    "INSERT IGNORE INTO outfit_image (id, outfit_hash, prompt_hash, image_path, prompt_text) VALUES (?, ?, ?, ?, ?)"
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&outfit_hash)
                .bind(&prompt_hash)
                .bind(&url)
                .bind(prompt.char_indices().nth(500).map_or(&prompt[..], |(i, _)| &prompt[..i]))
                .execute(&state.db)
                .await;
                final_url = Some(url);
                break;
            } else {
                tracing::warn!(
                    "gender check failed (attempt {}) — male detected, retrying",
                    attempt + 1
                );
                let _ = std::fs::remove_file(&path);
                if attempt == 2 {
                    // 마지막 시도도 실패하면 그냥 사용
                    tracing::warn!("all gender checks failed, using last image");
                    let filename2 = format!("{}.png", uuid::Uuid::new_v4());
                    let path2 = format!("static/images/{}", filename2);
                    let _ = std::fs::write(&path2, &bytes);
                    final_url = Some(format!("/static/images/{}", filename2));
                }
            }
        }
    }

    Ok(Json(ImageResponse {
        image_url: final_url,
    }))
}

// ─── 무드별 이미지 프롬프트 생성 ───
fn build_image_prompt(mood: &str, items: &str, hash: u64) -> String {
    let hairstyles = [
        "messy long waves with curtain bangs, effortless undone texture",
        "chin-length blunt bob, slightly tousled",
        "low loose bun with face-framing strands, relaxed and casual",
        "center-part shoulder-length hair, natural air-dried texture",
    ];
    let hair = hairstyles[(hash as usize) % hairstyles.len()];

    let base_face = format!(
        "naturally attractive feminine face, very small head proportions relative to body, soft feminine V-line face with smooth rounded jawline — never angular or square jaw, thin soft eyebrows. Hairstyle: {}.",
        hair
    );
    let base_body = "fashion model proportions — very small head relative to body (8.5-head proportion), tall and lean with long limbs, long legs, narrow waist, slim with subtle feminine curves, 175cm tall figure.";
    let base_avoid = "male model, masculine face, angular jaw, square jawline, sharp chin, masculine bone structure, ugly face, distorted face, distorted mouth, open mouth, awkward lip shape, big head, large head relative to body, ordinary pedestrian look, catalog pose, ecommerce posture, mannequin, stiff standing, symmetrical front pose, cropped body, cropped legs, tight framing, oversaturated colors, harsh lighting";

    match mood {
        "quiet_luxury" => format!(
            r#"Quiet luxury fashion photo of an effortlessly elegant young woman in her mid to late 20s. She must be female.

Face: {base_face} Barely-there makeup with luminous natural skin, composed serene expression, understated confidence. Gold minimal jewelry.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with impeccable fit, no logos, luxurious fabrics (cashmere, silk, fine wool, soft leather). Clean timeless silhouette, understated elegance. Every piece should whisper quality through texture and drape, never through branding.

Pose: composed graceful stride or standing with effortless poise, one hand in coat pocket or holding leather tote, quiet confident body language. Full body visible from head to shoes.

Aesthetic: shallow depth of field, soft muted neutral tones, gentle overcast daylight, clean modern architecture or private gallery entrance or quiet tree-lined residential street, understated luxury atmosphere. The Row / Loro Piana / soft luxury editorial mood.

Avoid: {base_avoid}, logos, bold patterns, streetwear elements, sporty pieces, romantic frills, oversaturated colors."#
        ),
        "coquette" => format!(
            r#"Coquette balletcore fashion photo of a charming young woman in her early 20s. She must be female.

Face: {base_face} Soft rosy dewy makeup with pink blush, gentle flirtatious expression with soft smile, pearl or ribbon accessories.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with intentional femininity: ribbons, lace trims, bows, pastel tones, ballet-inspired silhouettes. Slip dresses, puff sleeves, delicate layering. Clothing should feel romantic and playful, never childish.

Pose: graceful ballet-inspired moment, light on feet, one hand touching ribbon or adjusting hair, soft feminine body language with gentle movement. Full body visible from head to shoes.

Aesthetic: shallow depth of field, warm pink-golden soft tones, gentle afternoon sunlight, Parisian patisserie or flower market or pink-toned European streetscape, dreamy romantic atmosphere, soft bokeh. Miu Miu / Sandy Liang / balletcore Pinterest mood.

Avoid: {base_avoid}, masculine styling, dark heavy tones, oversized baggy fit, street edge, sporty elements."#
        ),
        "office_siren" => format!(
            r#"Office siren fashion photo of a sharp confident young professional woman in her mid 20s. She must be female.

Face: {base_face} Cool polished makeup with defined brows and subtle smoky eyes, sharp intelligent gaze with quiet power, modern glasses optional.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with sharp tailored silhouette, slim-fit blazer, pencil skirt or tailored trousers, structured proportions. Mix of power dressing with understated sensuality. Clean, pressed, intentional — more soft office than aggressive siren.

Pose: confident power stride or leaning against glass wall, one hand adjusting blazer or holding structured bag, composed commanding body language. Full body visible from head to shoes.

Aesthetic: shallow depth of field, cool neutral tones with warm highlights, soft morning daylight, modern glass office lobby or luxury hotel corridor or sleek city sidewalk, professional power atmosphere. Devil Wears Prada meets The Row. Soft corporate chic mood.

Avoid: {base_avoid}, casual sneakers, oversized baggy fit, vintage distressing, romantic frills, sporty elements."#
        ),
        "boho" => format!(
            r#"Luxury bohemian fashion photo of a free-spirited stylish young woman in her early 20s. She must be female.

Face: {base_face} Warm sun-kissed makeup with bronzed glow, relaxed dreamy expression, effortless bohemian beauty.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with refined bohemian silhouette: suede, fringe, flowing layers, ethnic-inspired details, warm earthy tones. More luxurious and urban than classic boho — 2026 boho revival is polished, not hippie.

Pose: free-spirited moment, walking through outdoor market or leaning on rustic doorframe, wind-blown hair movement, relaxed bohemian body language. Full body visible from head to shoes.

Aesthetic: shallow depth of field, warm golden earthy tones, golden hour afternoon sunlight, vintage flea market or terracotta-walled alleyway or desert-toned urban landscape, warm textured atmosphere. Isabel Marant / Free People elevated lookbook mood.

Avoid: {base_avoid}, minimal clean styling, corporate look, sporty elements, neon colors, tech fabrics."#
        ),
        "off_duty" => format!(
            r#"Off-duty model fashion photo of a wellness-chic young woman in her early 20s. She must be female.

Face: {base_face} Fresh dewy no-makeup look with healthy inner glow, calm confident expression, effortless model-off-duty radiance.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with elevated athleisure meets luxury street. Body-hugging fitted pieces balanced with relaxed oversized layers. Leggings and biker shorts show sculpted body line. Sports bra tops can be worn alone or layered. Muted neutral tones. The look should feel like a supermodel running errands, not going to the gym.

Pose: relaxed post-workout moment, calm confident stance, one hand holding iced coffee or yoga mat, natural walking, serene grounded body language. Full body visible from head to shoes.

Aesthetic: shallow depth of field, soft warm natural light, clean bright tones, Hangang riverside park or cafe terrace after workout or Seoul urban hiking trail, fresh green surroundings, wellness lifestyle atmosphere. Alo Yoga / adidas by Stella McCartney / model-off-duty Pinterest mood.

Avoid: {base_avoid}, formal styling, vintage distressing, dark moody tones, heavy makeup, aggressive gym energy, harsh lighting."#
        ),
        "street" => format!(
            r#"Urban street-style fashion photo of an energetic young woman in her late teens. She must be female.

Face: {base_face} Bold minimal makeup with strong brows, confident energetic expression.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with edgy street silhouette, curated mix of oversized and fitted, bold layering, urban cool energy. Hint of maximalism — intentional clash, not messy.

Pose: dynamic confident stance, weight on one leg, one hand in pocket or adjusting jacket, strong attitude and energy. Full body visible from head to shoes.

Aesthetic: shallow depth of field, high contrast muted tones, bright daylight, graffiti wall or skate park or urban concrete with street art, raw urban energy. Hypebeast / curated chaos street fashion mood.

Avoid: {base_avoid}, feminine soft styling, luxury campaign mood, romantic atmosphere, pastel tones."#
        ),
        "boyish" => format!(
            r#"Street-style fashion photo of a young boyish-cool woman in her early 20s. She must be female.

Face: {base_face} Minimal fresh makeup, cool confident expression with relaxed eyes.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with relaxed oversized fit, slightly baggy silhouette, effortless boyish gender-neutral styling. All clothing should look worn-in with subtle fading and vintage patina.

Pose: candid cool-girl moment, relaxed stance with hands in pockets, slight head tilt, laid-back confident expression. Full body visible from head to shoes.

Aesthetic: shallow depth of field, muted warm tones, bright afternoon sunlight, vintage shop front with old signage or narrow alleyway with weathered brick, hipster atmosphere. Pinterest street-style mood.

Avoid: {base_avoid}, feminine delicate styling, formal look, luxury campaign mood."#
        ),
        _ => format!(
            // amekaji / default — 기존 힙스터 스타일
            r#"Street-style fashion photo of a young hipster female fashion model in her early 20s with cool urban energy. She must be female.

Face: {base_face} Minimal fresh makeup, laid-back confident expression.

Body: {base_body}

Outfit: The female model is wearing {items} — styled with relaxed oversized fit, slightly baggy silhouette, effortless young urban hipster styling. All clothing should look worn-in with visible aging, subtle fading, soft washed texture, natural distressing, and vintage patina.

Pose: candid cool-girl moment, relaxed natural stance with weight on one leg, hands in pockets or holding coffee, slight head tilt, laid-back confident expression. Full body visible from head to shoes.

Aesthetic: shallow depth of field, soft cinematic grading, muted warm tones, bright natural afternoon sunlight, narrow alleyway with graffiti walls, old brick buildings, parked bicycles, weathered textures, hipster atmosphere. Pinterest street-style photography, Kinfolk magazine mood.

Avoid: {base_avoid}, tight-fitting clothes, formal styling, luxury campaign mood."#
        ),
    }
}

// ─── 성별 검증 (GPT-4o-mini vision) ───
async fn verify_female_model(llm: &LlmClient, b64_image: &str) -> bool {
    let req = LlmChatRequest::new(vec![Message::user_image(
        "Is the person in this photo female? Reply with only 'yes' or 'no'.",
        format!("data:image/png;base64,{}", b64_image),
    )]);

    match llm.chat(LlmTask::GenderVerify, req).await {
        Ok(resp) => resp.text_or_empty().to_lowercase().contains("yes"),
        Err(e) => {
            // 검수기가 죽었다고 생성 파이프라인을 막지는 않는다 — 통과시키고 로그를 남긴다.
            tracing::warn!("gender verification failed: {e}");
            true
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
