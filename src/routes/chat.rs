use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::db::{clothing_repo, feedback_repo};
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::models::feedback::FeedbackRequest;
// 라우트의 DTO(ChatRequest/ImageRequest)와 이름이 겹쳐 alias 한다.
use crate::AppState;
use crate::models::style_vocab::{StyleGenre, Tone, Weight};
use crate::services::llm::{
    ChatRequest as LlmChatRequest, ImageDetail, ImageEditRequest as LlmImageEditRequest,
    ImageRequest as LlmImageRequest, LlmClient, LlmTask, Message, ToolDef, Usage,
};
use crate::services::outfit_scorer;
use crate::services::weather as weather_service;

/// 사용자가 말한 아이템("올리브 슬립온")을 옷장 아이템과 임베딩으로 맞출 때의 최소 유사도.
///
/// 업로드 분석의 `prompts::RAG_MIN_SIMILARITY` 와 값은 같지만 **다른 기준이다.** 저쪽은
/// Vision 1차 서술 ↔ 레퍼런스 검색 문장이고, 이쪽은 짧은 대화 표현 ↔ 옷장 아이템 이름·
/// 색·소재를 이어 붙인 문장이다. 비교하는 글의 길이와 형식이 달라 유사도 분포도 다르다.
/// 한쪽을 보정했다고 다른 쪽에 같은 값을 쓰면 안 된다.
///
/// **카테고리를 모를 때만 쓴다.** 2026-10-11 질의 44개(옷장 333벌)로 쟀을 때 기준값으로는
/// 맞는 옷과 엉뚱한 옷이 갈리지 않았다 — 옷장에 정확히 있는 옷이 0.42 까지 내려가고,
/// 다른 카테고리 옷이 0.70 까지 올라간다(색이 같으면 걸린다). 카테고리를 알면 그 안의
/// 1위를 그대로 쓰고, 이 값은 카테고리를 모를 때 다른 카테고리로 새는 것을 일부 막는
/// 용도로만 남긴다(0.5 에서 다른 카테고리 3건 통과, 0.4 에서 7건).
const WARDROBE_MATCH_MIN_SIMILARITY: f32 = 0.5;
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(chat))
        .route("/image", post(generate_image))
        .route("/history", get(chat_history).delete(clear_history))
}

/// 남겨 둔 상담 한 줄.
#[derive(Debug, Serialize, sqlx::FromRow)]
struct HistoryRow {
    id: i64,
    role: String,
    text: String,
    #[sqlx(json(nullable))]
    items_json: Option<Vec<ChatItem>>,
    #[sqlx(json(nullable))]
    base_items_json: Option<Vec<ChatItem>>,
    mood_key: Option<String>,
    created_at: chrono::NaiveDateTime,
}

/// 최근 대화. 화면이 한 번에 들고 있을 만큼만, 오래된 것부터 돌려준다.
async fn chat_history(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<HistoryRow>>, AppError> {
    let mut rows = sqlx::query_as::<_, HistoryRow>(
        "SELECT id, role, text, items_json, base_items_json, mood_key, created_at \
         FROM chat_message WHERE user_id = ? ORDER BY id DESC LIMIT 100",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    rows.reverse();
    Ok(Json(rows))
}

/// 대화를 지운다. 사진(`outfit_image`)은 남긴다 — 같은 코디를 다시 만들면 돈이 든다.
async fn clear_history(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let done = sqlx::query("DELETE FROM chat_message WHERE user_id = ?")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(json!({ "deleted": done.rows_affected() })))
}

/// 질문과 답을 함께 남긴다. 답을 못 만든 질문은 남기지 않는다 — 다시 열었을 때 답 없는
/// 질문만 덩그러니 남는다. 저장 실패가 답을 막을 이유는 없어서 경고만 남긴다.
async fn save_exchange(
    db: &sqlx::MySqlPool,
    user_id: &str,
    body: &ChatRequest,
    reply: &str,
    items: &[ChatItem],
) {
    let items_json = (!items.is_empty()).then(|| json!(items));
    let base_json =
        (!items.is_empty() && !body.previous_items.is_empty()).then(|| json!(body.previous_items));
    let saved = sqlx::query(
        "INSERT INTO chat_message (user_id, role, text, items_json, base_items_json, mood_key) \
         VALUES (?, 'user', ?, NULL, NULL, ?), (?, 'ai', ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(&body.message)
    .bind(&body.style_mood)
    .bind(user_id)
    .bind(reply)
    .bind(items_json)
    .bind(base_json)
    .bind(&body.style_mood)
    .execute(db)
    .await;
    if let Err(e) = saved {
        tracing::warn!("상담 기록 저장 실패: {e}");
    }
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    style_mood: Option<String>,
    /// 바로 앞에 보여 준 코디. 있으면 "신발만 바꿔줘" 같은 말을 이 코디에 대한 부분
    /// 수정으로 다룬다. 상담은 대화를 서버에 남기지 않으므로 화면이 넘겨 준다.
    #[serde(default)]
    previous_items: Vec<ChatItem>,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    reply: String,
    items: Vec<ChatItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatItem {
    slot: String,
    category: String,
    name: String,
    #[serde(default)]
    owned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<String>,
    /// 착용 상태(영문, 이미지 입력에 붙는다). 추운 날 코트는 여며 입은 상태로 보온을
    /// 판정하므로(`serving_ranker::outer_wear`) 그림도 같은 상태로 그린다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wear: Option<String>,
}

/// 기온을 알면 아우터 칸에 착용 상태를 붙인다. 바꾸기(`tool_swap_item`)로 들어온 아우터도
/// 같은 기준을 받도록 응답 직전에 한 번 더 부른다.
fn apply_wear(items: &mut [ChatItem], clothes: &[Clothing], temperature: Option<f64>) {
    let Some(t) = temperature else { return };
    for item in items.iter_mut().filter(|i| i.slot == "outer") {
        item.wear = clothes
            .iter()
            .find(|c| c.name == item.name)
            .and_then(|c| crate::services::serving_ranker::outer_wear(c, t))
            .map(str::to_string);
    }
}

/// 옷장 카테고리 → 상담 카드의 슬롯 이름. 화면(`lib/lookbook.ts` 의 `CATEGORY_TO_SLOT`)과 같다.
fn slot_for_category(category: &str) -> Option<&'static str> {
    match category {
        "상의" => Some("inner"),
        "아우터" => Some("outer"),
        "하의" => Some("bottom"),
        "신발" => Some("shoes"),
        "가방" => Some("bag"),
        _ => None,
    }
}

impl ChatItem {
    /// 옷장 아이템 하나를 카드 한 칸으로. 슬롯이 없는 카테고리면 `None`.
    fn from_clothing(c: &Clothing, owned: bool) -> Option<Self> {
        Some(Self {
            slot: slot_for_category(&c.category)?.to_string(),
            category: c.category.clone(),
            name: c.name.clone(),
            owned,
            material: c.image_material(),
            wear: None,
        })
    }
}

/// 상담 모델에 주는 도구. provider 중립 형태다 — OpenAI 의 `function.parameters` 든
/// Anthropic 의 `input_schema` 든 직렬화는 provider 구현체가 한다. `swap_item` 은 직전
/// 코디가 있을 때만 따로 더한다.
fn chat_tools() -> Vec<ToolDef> {
    vec![
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
            "유저가 특정 옷을 지정했을 때, 그 anchor 아이템 기준으로 서버가 최적의 착장을 생성한다. user_query는 유저 원문, anchor_name은 search_wardrobe에서 찾은 정확한 DB 이름. 특정 옷 없이 전체 코디를 원하면 get_outfit_for_today 를 쓴다.",
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
            "get_outfit_for_today",
            "기준 아이템 없이, 오늘 날씨와 화면에서 고른 무드에 맞는 코디를 서버가 옷장에서 만든다. 유저가 특정 옷을 말하지 않고 오늘 입을 코디를 원할 때 쓴다. 기준 옷도 서버가 고른다.",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDef::new(
            "evaluate_outfit",
            "서버가 착장 조합의 품질을 검증한다. 문제가 있으면 이유와 함께 실패를 반환한다. get_outfit·get_outfit_for_today 결과를 검증할 때 사용.",
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
    ]
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
    let mood = crate::routes::parse_genre(body.style_mood.as_deref())?;
    let clothes = if body.gender.is_some() || body.style_mood.is_some() {
        clothing_repo::list_clothing_filtered(&state.db, body.gender.as_deref(), mood).await?
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

    // 최근 추천 이력 — 같은 조합을 계속 내놓지 않게 감점한다.
    //
    // 홈 추천은 이력을 읽어 반복을 눌러 왔는데 채팅은 점수 1등을 그대로 돌려줬다.
    // 옷장과 날씨가 그대로면 점수도 그대로이므로, 같은 앵커로 다시 물으면 매번
    // 같은 착장이 나왔다.
    let recent = outfit_scorer::RecentHistory::from_history(
        &RecommendationHistoryRepo::find_recent_by_user(&state.db, user_id, 10)
            .await
            .unwrap_or_default(),
    );

    let tools = chat_tools();

    let has_previous = !body.previous_items.is_empty();

    // 옷 자체를 고쳐 달라는 말("맨투맨에 스누피 그래픽 추가해줘")은 옷장 아이템을 바꾸는
    // 이 기능으로 할 수 없다. 모델에 맡기면 비슷한 옷으로 바꾸거나 다른 칸까지 손대서,
    // 사진은 그대로인데 착장만 엉뚱하게 바뀐다. 모델을 부르지 않고 바로 알린다.
    if has_previous && asks_to_modify_item(&body.message) {
        let reply = "지금은 옷장에 있는 다른 옷으로 바꾸는 것만 할 수 있어요. 그래픽이나 자수처럼 옷 자체를 고치는 건 아직 못 해요.".to_string();
        save_exchange(&state.db, user_id, &body, &reply, &[]).await;
        return Ok(Json(ChatResponse {
            reply,
            items: Vec::new(),
        }));
    }
    // 유저가 말한 카테고리. 비어 있으면("좀 더 화사하게") 어느 칸이든 바꿀 수 있다.
    let asked_categories = mentioned_categories(&body.message);
    // 부분 수정은 유저가 콕 집어 말한 것을 찾는 일이라, 무드 안에 없으면 같은 성별의
    // 옷장 전체에서 찾는다 ("카멜 신발" 이 미니멀 무드에 없다고 흰 스니커를 주면 안 된다).
    let wide_clothes = if has_previous && body.style_mood.is_some() {
        clothing_repo::list_clothing_filtered(&state.db, body.gender.as_deref(), None).await?
    } else {
        Vec::new()
    };
    let mut tools = tools;
    if has_previous {
        tools.push(ToolDef::new(
            "swap_item",
            "직전에 보여 준 코디에서 한 카테고리를 옷장의 다른 아이템으로 바꾼다. 나머지 아이템은 그대로 둔다. 유저가 원하는 것이 있으면 query 로 넘긴다(예: 로퍼, 검은 신발). 없으면 비워 두면 서버가 나머지와 가장 잘 맞는 것을 고른다. 지금 입은 옷에 그래픽·무늬·자수·로고를 더하거나 핏·기장을 고치는 '옷 자체의 수정'은 이 도구로 할 수 없다 — 그런 요청이면 호출하지 말고, 옷장에 있는 다른 옷으로 바꾸는 것만 된다고 한 문장으로 알려라.",
            json!({
                "type": "object",
                "properties": {
                    "category": { "type": "string", "enum": ["상의","하의","아우터","신발","가방"], "description": "바꿀 카테고리" },
                    "query": { "type": "string", "description": "유저가 원하는 아이템 설명. 없으면 빈 문자열" }
                },
                "required": ["category"]
            }),
        ));
    }
    let previous_hint = if has_previous {
        format!(
            "\n\n직전에 보여 준 코디:\n{}\n유저가 이 코디의 일부만 바꾸자고 하면(예: '신발만 바꿔줘', '가방 다른 걸로') get_outfit·get_outfit_for_today 대신 swap_item 을 바꿀 카테고리마다 한 번씩 호출하라. 새 코디를 원하면 특정 옷을 말했을 때는 get_outfit, 아니면 get_outfit_for_today 를 쓴다.\nswap_item 을 쓴 뒤에는 get_outfit·get_outfit_for_today 를 부르지 마라 — 유저가 바꾸라고 한 곳만 바꾼다. evaluate_outfit 이 아쉬운 점을 말해도 같은 카테고리 안에서 swap_item 을 한 번 더 부르거나, 그대로 두고 그 점을 한 문장으로 알려라.\nswap_item 의 query 에는 유저가 말한 표현만 넣어라 — 유저가 말하지 않은 색이나 소재를 붙이지 마라.\n답변은 swap_item 이 돌려준 changed.to 의 실제 이름으로, 바뀐 아이템이 나머지와 어떻게 어울리는지 착장 설명 규칙대로 2문장 쓴다. 아쉬운 점은 있을 때만 마지막에 한 문장.",
            body.previous_items
                .iter()
                .map(|i| format!("- {}: {}", i.category, i.name))
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        String::new()
    };

    // 화면에서 고른 무드. 예전에는 서버가 옷장을 거르는 데만 쓰고 모델에는 알리지 않아서,
    // "엣지 무드로 추천해줘" 같은 요청에도 모델이 무드를 모른 채 기준 아이템을 되물었다
    // (2026-10-17 실측).
    let mood_hint = match mood {
        Some(m) => {
            let label: Option<(String,)> = sqlx::query_as(
                "SELECT mood_label FROM style_mood WHERE mood_key = ? \
                 ORDER BY gender = ? DESC LIMIT 1",
            )
            .bind(m.as_str())
            .bind(body.gender.as_deref().unwrap_or(""))
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            let label = label.map(|r| r.0).unwrap_or_else(|| m.as_str().to_string());
            format!(
                "\n선택한 무드: {label}. 옷장 검색과 코디는 서버가 이미 이 무드의 옷장 안에서 고른다."
            )
        }
        None => String::new(),
    };

    let system_prompt = format!(
        r#"너는 프리미엄 셀렉샵 에디토리얼 스타일리스트다. AURALEE, BEAMS, HAVEN 같은 감성으로 코디를 설명한다.

역할:
- 유저의 질문을 이해하고, 도구를 호출해서 답변한다.
- 코디 추천은 서버가 결정한다. 직접 아이템을 고르지 마라. 유저가 특정 옷을 지정하면 get_outfit, 특정 옷 없이 전체 코디를 원하면 get_outfit_for_today 를 쓴다.
- anchor 아이템을 찾을 때는 search_wardrobe를 호출한다.
- 유저가 싫다/좋다 등 피드백을 주면 submit_feedback을 호출한다.

흐름:
0. 유저가 특정 아이템을 말하지 않고 오늘 입을 코디를 원하면(예: '오늘 날씨에 맞게 추천해줘', '이 무드로 코디해줘') 아이템을 되묻지 말고 get_outfit_for_today 를 호출한다. 무엇을 원하는지 자체가 불분명할 때만 질문한다
1. 유저가 아이템을 언급하면 → search_wardrobe로 anchor 찾기
2. anchor가 확정되면 → get_outfit으로 서버 추천 받기
3. get_outfit·get_outfit_for_today 결과를 evaluate_outfit으로 검증
4. 검증 통과 → 착장을 설명
5. 검증 실패 → get_outfit 경로면 avoid_tags와 함께 get_outfit 재호출. get_outfit_for_today 경로면 다시 불러도 같은 코디가 나오므로 재호출하지 말고, 아쉬운 점을 한 문장으로 알린다
6. 유저가 피드백 주면 → submit_feedback 후, get_outfit 경로면 get_outfit 재호출. get_outfit_for_today 경로는 같은 요청 안에서 다시 불러도 같은 코디라 재호출하지 말고, 피드백이 다음 추천에 반영된다고 알린다

착장 설명 규칙 (중요):
- 아이템 리스트를 나열하지 마라 (UI에서 이미 표시됨)
- '색상 조화가 좋습니다' 같은 generic 표현 금지
- texture(질감), silhouette(실루엣), visual weight(시각적 무게), grounding(하체 안정감) 중심으로 2~3문장 작성
- 마크다운/볼드/리스트 없이 순수 텍스트로 답변
- 예시: '블루종의 드라이한 면 질감이 상체를 부드럽게 정리하고, 린넨 셔츠가 레이어링에 가벼운 깊이를 만듭니다. 캔버스 스니커로 하체 대비를 잡았습니다.'

날씨: {weather}{mood_hint}
답변은 한국어로.{previous_hint}"#,
        weather = if weather_hint.is_empty() {
            "정보 없음".to_string()
        } else {
            weather_hint.clone()
        },
        mood_hint = mood_hint,
    );

    // ─── Tool calling loop (최대 8회 반복) ───
    let mut messages: Vec<Message> = vec![Message::user_text(body.message.clone())];
    let mut final_items: Vec<ChatItem> = Vec::new();
    let mut final_item_ids: Vec<String> = Vec::new();
    let mut final_reply = String::new();
    let mut first_search_query: Option<String> = None; // 유저 최초 검색어 (덮어쓰기 불가)
    let mut anchor_category: Option<String> = None;
    // 이번 요청에서 swap_item 으로 일부를 바꿨는지. 그 뒤로는 전체 재구성을 막는다.
    let mut swapped = false;
    // 유저 요청으로 바꾼 카테고리. 평가가 나온 뒤에는 이 밖의 카테고리를 바꾸지 못한다 —
    // 평가가 "하체가 가볍다" 고 하면 모델이 묻지도 않은 신발을 바꾼다 (2026-10-15).
    let mut swapped_categories: Vec<String> = Vec::new();
    let mut evaluated = false;

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
                    // 부분 수정 중에는 코디를 통째로 새로 짜지 않는다. 지시문으로도 막지만,
                    // 평가가 아쉬운 점을 말하면 모델이 "검증 실패 → get_outfit 재호출" 흐름을
                    // 따라가 버린다 (2026-10-15: '신발만 로퍼로' 가 하의·가방까지 바뀌었다).
                    "get_outfit" | "get_outfit_for_today" if swapped => json!({
                        "error": "유저는 일부만 바꾸길 원한다. 전체 코디를 새로 짜지 말고 swap_item 결과를 그대로 설명하라."
                    })
                    .to_string(),
                    "get_outfit_for_today" => {
                        let (outfit_json, items, chosen_ids) = today_outfit(
                            &clothes,
                            &OutfitContext {
                                user: user_profile.as_ref(),
                                temperature,
                                feedback: &feedback_ctx,
                                recent: &recent,
                                mood,
                            },
                        );
                        final_items = items;
                        final_item_ids = chosen_ids;
                        outfit_json
                    }
                    "get_outfit" => {
                        let user_query = fn_args["user_query"]
                            .as_str()
                            .unwrap_or(fn_args["anchor_name"].as_str().unwrap_or(""));
                        let anchor_name = fn_args["anchor_name"].as_str().unwrap_or(user_query);
                        let (outfit_json, mut items, chosen_ids) = tool_get_outfit(
                            user_query,
                            anchor_name,
                            anchor_category.as_deref(),
                            &clothes,
                            &OutfitContext {
                                user: user_profile.as_ref(),
                                temperature,
                                feedback: &feedback_ctx,
                                recent: &recent,
                                mood,
                            },
                            &state.embedding,
                        )
                        .await;
                        // 유저 원문으로 anchor 슬롯 즉시 교체
                        if let Some(ref uq) = first_search_query {
                            let is_in_db = clothes.iter().any(|c| c.name == *uq);
                            if !is_in_db && let Some(ref cat) = anchor_category {
                                let sk = slot_for_category(cat).unwrap_or("");
                                if let Some(item) = items.iter_mut().find(|i| i.slot == sk) {
                                    item.name = uq.clone();
                                    item.owned = false;
                                }
                            }
                        }
                        final_items = items;
                        final_item_ids = chosen_ids;
                        outfit_json
                    }
                    "swap_item"
                        if !swap_allowed(
                            fn_args["category"].as_str().unwrap_or(""),
                            &asked_categories,
                            &swapped_categories,
                            evaluated,
                        ) =>
                    {
                        json!({
                            "error": "유저가 바꾸라고 하지 않은 카테고리다. 바꾸지 말고, 지금 착장을 설명하라."
                        })
                        .to_string()
                    }
                    "swap_item" => {
                        let category = fn_args["category"].as_str().unwrap_or("");
                        let query = fn_args["query"].as_str().unwrap_or("").trim();
                        // 두 번째 교체는 첫 번째 결과 위에 한다 — '신발이랑 가방' 처럼.
                        let base = if final_items.is_empty() {
                            body.previous_items.clone()
                        } else {
                            final_items.clone()
                        };
                        let from = base
                            .iter()
                            .find(|i| i.category == category)
                            .map(|i| i.name.clone());
                        match tool_swap_item(
                            &base,
                            category,
                            query,
                            &clothes,
                            &wide_clothes,
                            &state.embedding,
                            mood,
                            temperature,
                        )
                        .await
                        {
                            Some((items, ids)) => {
                                swapped = true;
                                if !swapped_categories.iter().any(|c| c == category) {
                                    swapped_categories.push(category.to_string());
                                }
                                let to = items
                                    .iter()
                                    .find(|i| i.category == category)
                                    .map(|i| i.name.clone());
                                let items_json: Vec<serde_json::Value> = items
                                    .iter()
                                    .map(|i| json!({"slot": i.slot, "name": i.name, "category": i.category}))
                                    .collect();
                                final_items = items;
                                final_item_ids = ids;
                                // 무엇이 무엇으로 바뀌었는지 따로 적는다. 목록만 주면 모델이
                                // 자기가 검색한 말("블랙 로퍼")로 답해 실제 아이템과 어긋난다.
                                json!({
                                    "changed": { "category": category, "from": from, "to": to },
                                    "items": items_json,
                                })
                                .to_string()
                            }
                            None => json!({
                                "error": "옷장에 유저가 말한 아이템이 없다. 바꾸지 말고, 없다는 것을 유저에게 한 문장으로 알려라."
                            })
                            .to_string(),
                        }
                    }
                    "evaluate_outfit" => {
                        evaluated = true;
                        let names: Vec<String> = fn_args["item_names"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        tool_evaluate_outfit(&names, &clothes, user_profile.as_ref(), mood)
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

    // 돌려준 착장을 이력에 남긴다.
    //
    // 남기지 않으면 위의 감점이 홈 추천 이력만 보게 되고, 채팅은 자기가 방금
    // 내놓은 착장을 다음 질문에서 또 내놓는다. 표가 하나이므로 홈 추천도 이걸
    // 같이 보게 된다 — 옷장은 하나이고, "어제 입은 옷"이 어느 화면에서 나왔는지는
    // 사용자에게 중요하지 않다.
    if !final_item_ids.is_empty() {
        // 사용자 원문으로 바꿔 보여 준 슬롯(미보유)은 뺀다. 그 자리의 아이템은
        // 점수 계산에만 쓴 대역이고 추천한 적이 없으므로, 남기면 보여 주지도 않은
        // 옷에 반복 감점이 붙는다.
        let slot_id = |cat: &str, slot: &str| -> Option<String> {
            if !final_items.iter().any(|i| i.slot == slot && i.owned) {
                return None;
            }
            final_item_ids
                .iter()
                .find(|id| clothes.iter().any(|c| c.id == **id && c.category == cat))
                .cloned()
        };
        let _ = RecommendationHistoryRepo::insert(
            &state.db,
            user_id,
            slot_id("상의", "inner").as_deref(),
            slot_id("하의", "bottom").as_deref(),
            slot_id("아우터", "outer").as_deref(),
            slot_id("신발", "shoes").as_deref(),
            slot_id("가방", "bag").as_deref(),
        )
        .await;
    }

    apply_wear(&mut final_items, &clothes, temperature);
    save_exchange(&state.db, user_id, &body, &final_reply, &final_items).await;

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
    /// 바로 앞에 보여 준 코디(같은 형식의 문자열). 상담에서 "신발만 바꿔줘" 처럼
    /// 일부만 바뀐 코디라면, 앞 사진을 편집해 같은 모델·포즈·배경으로 보여 준다.
    #[serde(default)]
    base_items: Option<String>,
}

#[derive(Debug, Serialize)]
struct ImageResponse {
    image_url: Option<String>,
}

/// 이미지 캐시 키.
///
/// `outfit_image` 는 (outfit_hash, prompt_hash) 로 걸린다. 저장해 둔 룩의 그림을
/// 찾을 때도 같은 값이 필요해서, 계산을 한 곳에 둔다 — 두 곳에서 따로 계산하면
/// 규칙이 어긋나는 순간 캐시가 조용히 빗나가고 같은 그림을 다시 만든다.
pub fn image_cache_keys(items: &str, genre: StyleGenre) -> (String, String, u64, String) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut outfit_hasher = DefaultHasher::new();
    items.hash(&mut outfit_hasher);
    let outfit_hash_val = outfit_hasher.finish();

    let prompt = build_image_prompt(genre, items, outfit_hash_val);
    let mut prompt_hasher = DefaultHasher::new();
    prompt.hash(&mut prompt_hasher);

    (
        format!("{outfit_hash_val:016x}"),
        format!("{:016x}", prompt_hasher.finish()),
        outfit_hash_val,
        prompt,
    )
}

async fn generate_image(
    State(state): State<AppState>,
    Json(body): Json<ImageRequest>,
) -> Result<Json<ImageResponse>, AppError> {
    // 캐시 체크: outfit_hash + prompt_hash
    // 이미지 프롬프트는 장르마다 다르다. 모르는 값이면 400 으로 돌려준다 —
    // 엉뚱한 장르의 이미지를 만들어 캐시에 넣는 것보다 낫다.
    let genre = crate::routes::parse_genre(body.mood.as_deref())?.unwrap_or(StyleGenre::Amekaji);
    let (outfit_hash, prompt_hash, _, prompt) = image_cache_keys(&body.items, genre);

    // 룩북에 담아 둔 코디면 그때의 사진을 그대로 쓴다. 프롬프트를 고쳐 캐시 키가
    // 바뀌어도 담아 둔 사진이 새 그림으로 바뀌거나 비면 안 된다.
    if let Some(path) =
        crate::routes::lookbook::pinned_image(&state.db, &body.items, body.mood.as_deref()).await
    {
        return Ok(Json(ImageResponse {
            image_url: Some(path),
        }));
    }

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

    // 앞 사진에서 1~2개만 바뀌었으면 편집부터 해 본다. 실패하면 아래에서 새로 만든다.
    if let Some(base_items) = body.base_items.as_deref()
        && let Some(url) = edit_from_base(&state, genre, base_items, &body.items, &mut m).await
    {
        outcome = Some((Some(url), VerificationStatus::Passed));
    }
    let generate_attempts = if outcome.is_some() {
        0
    } else {
        MAX_IMAGE_ATTEMPTS
    };

    for attempt in 1..=generate_attempts {
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

    // 사진 없이 담아 둔 룩이면 이 사진을 붙인다.
    if let Some(path) = &url {
        crate::routes::lookbook::pin_image(&state.db, &body.items, body.mood.as_deref(), path)
            .await;
    }

    Ok(Json(ImageResponse { image_url: url }))
}

/// 편집 입력 이미지 토큰의 단가 차이 (이미지 $8 − 단가표의 텍스트 $5, 100만 토큰당).
const EDIT_IMAGE_INPUT_SURCHARGE_PER_MTOK: f64 = 3.0;

/// 편집으로 바꿀 수 있는 아이템 수. 이보다 많이 바뀌면 사실상 다른 코디라 새로 만든다.
const MAX_EDIT_CHANGES: usize = 2;

/// 이미지용 코디 문자열(`"top/inner: 이름 (재질), shoes: 이름 (재질)"`)을 슬롯별로 나눈다.
///
/// 재질 괄호 안에도 쉼표가 있어서(`(leather, open back)`) 괄호 밖의 쉼표만 자른다.
fn split_image_items(items: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut push = |part: &str| {
        if let Some((slot, desc)) = part.split_once(':') {
            out.push((slot.trim().to_string(), desc.trim().to_string()));
        }
    };
    for (i, ch) in items.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                push(&items[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&items[start..]);
    out
}

/// 앞 코디에서 무엇이 바뀌었는지.
#[derive(Debug, PartialEq)]
enum ItemChange {
    Replace {
        slot: String,
        from: String,
        to: String,
    },
    Add {
        slot: String,
        to: String,
    },
    Remove {
        slot: String,
        from: String,
    },
}

fn diff_image_items(base: &str, next: &str) -> Vec<ItemChange> {
    let base = split_image_items(base);
    let next = split_image_items(next);
    let mut changes = Vec::new();
    for (slot, to) in &next {
        match base.iter().find(|(s, _)| s == slot) {
            Some((_, from)) if from == to => {}
            Some((_, from)) => changes.push(ItemChange::Replace {
                slot: slot.clone(),
                from: from.clone(),
                to: to.clone(),
            }),
            None => changes.push(ItemChange::Add {
                slot: slot.clone(),
                to: to.clone(),
            }),
        }
    }
    for (slot, from) in &base {
        if !next.iter().any(|(s, _)| s == slot) {
            changes.push(ItemChange::Remove {
                slot: slot.clone(),
                from: from.clone(),
            });
        }
    }
    changes
}

fn slot_noun(slot: &str) -> &str {
    match slot {
        "top/inner" => "top",
        "pants" => "bottoms",
        "outerwear" => "outer layer",
        other => other,
    }
}

/// 편집 지시문. 바꿀 것만 말하고, 나머지는 하나하나 그대로 두라고 못 박는다 —
/// 편집 모델은 "자연스럽게" 하려다 얼굴이나 다른 옷의 무늬까지 손대기 쉽다.
fn build_edit_prompt(changes: &[ItemChange]) -> String {
    let lines: Vec<String> = changes
        .iter()
        .map(|c| match c {
            ItemChange::Replace { slot, from, to } => {
                format!("replace her {} ({from}) with {to}", slot_noun(slot))
            }
            ItemChange::Add { slot, to } => format!("add {to} as her {}", slot_noun(slot)),
            ItemChange::Remove { slot, from } => format!(
                "take off her {} ({from}) and show what she wears underneath",
                slot_noun(slot)
            ),
        })
        .collect();
    format!(
        "Edit this fashion photo. Change only this: {}. \
         The new item sits naturally on her body, with the same light and shadows as the rest of the photo. \
         Keep everything else exactly as it is: the same woman, face, hair and expression, \
         the same pose and hands, every other garment and accessory with its colour, pattern, \
         texture and any logo unchanged, and the same background, light and framing.",
        lines.join("; ")
    )
}

/// 앞 사진을 편집해 이번 코디의 사진을 만든다. 쓸 수 없으면 `None` — 새로 생성한다.
///
/// 편집해도 여성 검사는 다시 한다. 모델이 그대로여야 하지만, 검사를 건너뛰면 "통과하지
/// 못한 그림은 내보내지 않는다" 는 보장이 편집 경로에서만 깨진다.
async fn edit_from_base(
    state: &AppState,
    genre: StyleGenre,
    base_items: &str,
    items: &str,
    m: &mut ImageGenMetrics,
) -> Option<String> {
    let changes = diff_image_items(base_items, items);
    if changes.is_empty() || changes.len() > MAX_EDIT_CHANGES {
        return None;
    }

    let (base_outfit_hash, base_prompt_hash, _, _) = image_cache_keys(base_items, genre);
    let base_path: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT image_path FROM outfit_image \
         WHERE outfit_hash = ? AND prompt_hash = ? AND image_path IS NOT NULL LIMIT 1",
    )
    .bind(&base_outfit_hash)
    .bind(&base_prompt_hash)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let base_url = base_path.and_then(|r| r.0)?;
    let base_png = std::fs::read(base_url.trim_start_matches('/')).ok()?;

    let started = std::time::Instant::now();
    let edited = state
        .llm
        .edit_image(&LlmImageEditRequest {
            base_png,
            prompt: build_edit_prompt(&changes),
            size: "1024x1536".to_string(),
            quality: "low".to_string(),
        })
        .await;
    m.generate_elapsed += started.elapsed();
    m.edit_calls += 1;
    let edited = match edited {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("image edit failed — generating instead: {e}");
            return None;
        }
    };
    m.edit_usage.input_tokens += edited.usage.input_tokens;
    m.edit_usage.output_tokens += edited.usage.output_tokens;

    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(edited.b64_png.as_str())
        .ok()?;
    let filename = format!("{}.png", uuid::Uuid::new_v4());
    let path = format!("static/images/{filename}");
    std::fs::write(&path, &bytes).ok()?;

    let started = std::time::Instant::now();
    let (check, usage) = verify_female_model(&state.llm, &edited.b64_png).await;
    m.verify_elapsed += started.elapsed();
    m.verify_calls += 1;
    m.verify_usage.input_tokens += usage.input_tokens;
    m.verify_usage.output_tokens += usage.output_tokens;

    if !matches!(next_step(check, MAX_IMAGE_ATTEMPTS), NextStep::Use) {
        tracing::warn!("edited image failed the gender check — generating instead");
        let _ = std::fs::remove_file(&path);
        return None;
    }

    tracing::info!(
        event = "outfit_image_edit",
        base = %base_url,
        changes = changes.len(),
        "앞 사진을 편집해 새 코디 사진을 만들었다: {path}"
    );
    Some(format!("/static/images/{filename}"))
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
    /// 앞 사진을 편집한 횟수. 모델과 단가가 생성과 달라 따로 센다.
    edit_calls: u32,
    generate_usage: Usage,
    verify_usage: Usage,
    edit_usage: Usage,
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
        let e = self.edit_cost_usd(llm);
        match (g, v, e) {
            (Some(g), Some(v), Some(e)) => Some(g + v + e),
            _ => None,
        }
    }

    /// 편집 비용. 단가표는 이미지 입력을 텍스트 단가($5)로 셈하는데, 편집은 입력의
    /// 대부분이 바탕 그림(이미지 단가 $8)이다. 그 차이를 더해 과소 추정을 막는다.
    fn edit_cost_usd(&self, llm: &LlmClient) -> Option<f64> {
        use crate::services::llm::usage::estimate_cost_usd;
        if self.edit_calls == 0 {
            return Some(0.0);
        }
        let base = estimate_cost_usd(
            &llm.config().task(LlmTask::ImageEdit).model,
            &self.edit_usage,
        )?;
        Some(
            base + self.edit_usage.input_tokens as f64 / 1_000_000.0
                * EDIT_IMAGE_INPUT_SURCHARGE_PER_MTOK,
        )
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
        .bind(self.generate_calls + self.edit_calls)
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
            edit_calls = self.edit_calls,
            edit_cost_usd = self.edit_cost_usd(llm),
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
/// 모든 무드가 함께 쓰는 장소. 무드별 장소만 쓰면 보헤미안은 늘 따뜻한 벽, 스포티는
/// 늘 운동장, 스트리트는 늘 회색 골목으로 굳는다(2026-10-15 사용자 지적). 무드는 옷이
/// 전하고, 배경은 그 옷을 보기 좋게 보여 주는 자리라서, 밝고 공간감 있는 도심 장소를
/// 함께 쓴다. 무드별 장소는 가중치 2, 여기는 1로 섞는다.
const SHARED_SETTINGS: [&str; 6] = [
    "a sunlit cafe terrace with a plain awning",
    "bright stone steps in front of a public building",
    "a riverside promenade with open sky",
    "a quiet street of pale stone buildings",
    "a sunlit plaza where light and shadow cross the paving",
    "a courtyard with a few potted plants and a rendered wall",
];

/// 배경이 무드를 전하는 방식. 장소가 아니라 배경의 소재·색·빛·구도로 차이를 낸다
/// (2026-10-15 사용자 제안). 경계가 가까운 무드는 여기서 갈린다 — 미니멀은 여백과
/// 절제, 모던 시크는 선과 대비. 클래식은 정돈된 구도, 매니시는 조금 더 느슨한 장면.
/// 로맨틱은 배경까지 달콤하게 만들지 않고, 꽃이나 곡선은 한 요소만 둔다.
fn genre_backdrop(genre: StyleGenre) -> &'static str {
    use StyleGenre as G;
    match genre {
        G::Minimal => "generous empty space, simple lines, and light and shadow doing the work",
        G::ModernChic => "crisp lines and strong contrast, with an urban sense of space",
        G::Classic => "materials with depth and a steady, orderly composition",
        G::Mannish => "plain, honest textures and a relaxed, roomy scene",
        G::Romantic => {
            "soft light, gentle curves and quiet colour — at most one curved or floral element, \
             never pink, flowers and blossom all at once"
        }
        G::Bohemian => "warm, sun-worn materials and a lived-in feel",
        G::Street => "colour, light and a sense of space in the city, not only rough grey",
        G::SportyCasual => "a bright, polished city day, nothing to do with training",
        // 글램은 밤·클럽·네온으로 굳히지 않는다. 배경은 결이 있는 재료 하나로 조용히 두고,
        // 빛을 받는 것은 옷이 맡는다.
        G::Glam => {
            "one quietly textured material — stone, glass or warm lamplight — kept calm \
             so the fabric of the clothes is what catches the light"
        }
        // 엣지는 회색 골목·그래피티·네온으로 굳히지 않는다. 선이 분명한 건축 요소나 색이 있는
        // 면 하나가 옷의 형태를 받쳐 준다.
        G::Edgy => {
            "one clear architectural line or a single field of colour with a sharp edge — \
             never a dark grungy alley, graffiti or neon"
        }
        // 팝: 색이 있는 면이나 단순한 그래픽 건축 요소 하나. 옷의 색·무늬보다 강하지 않게.
        G::Pop => {
            "one clear field of colour or a simple graphic architectural detail that suits the \
             colours of the clothes, calm enough that the pattern of the clothes stays the strongest"
        }
        _ => "a material and colour that suit the clothes",
    }
}

/// 모든 무드가 고르게 쓰는 빛. 무드마다 빛을 하나로 정해 두면 특정 무드만 예쁜 빛을
/// 독점한다. 무드의 빛 하나에 이 넷을 더해, 밝은 장면·그늘진 장면·따뜻한 장면이
/// 어느 무드에서나 나오게 한다.
const SHARED_LIGHTS: [&str; 4] = [
    "bright open daylight",
    "soft open shade from the trees or a building, with even light on her face",
    "warm late-afternoon sun from the side",
    "soft overcast light with a clear direction",
];

/// 빛을 고른다. 해시의 40~47비트를 쓴다.
fn pick_light(genre: StyleGenre, hash: u64) -> &'static str {
    let (_, own) = genre_setting(genre);
    let mut lights = vec![own];
    lights.extend(SHARED_LIGHTS.iter().copied().filter(|l| *l != own));
    lights[((hash >> 40) & 0xFF) as usize % lights.len()]
}

/// 자세 후보. 장르마다 자세를 하나씩 박아 두었더니 시선과 몸 각도까지 비슷해져 같은
/// 틀로 찍어 낸 사진처럼 보였다(2026-10-15 사용자 지적). 장르와 상관없이 다섯 중 하나를
/// 고른다. 편안한 눈맞춤도 자연스러우므로 카메라를 보는 자세도 둔다 — 예전의
/// "not posing for the photographer" 를 세게 걸면 옆을 보며 걷는 장면으로 수렴한다.
///
/// 걷는 자세 말고는 두 발을 디딘 채 서 있다고 못 박는다. 몸을 튼 자세에 걸음이 섞이면
/// 발끝은 한쪽을 보는데 앞발은 반대쪽에 놓여 두 다리가 엇갈린 그림이 나왔다
/// (2026-10-16 사용자 지적). 같은 이유로 "발을 살짝 교차" 자세는 뺐다.
const POSES: [&str; 5] = [
    "a natural near-frontal stance, standing still with both feet planted, weight on one leg, \
     eyes resting near the camera",
    "standing still, body turned twenty to thirty degrees from the camera with both feet \
     planted and pointing the same way as her hips, face turned back towards it",
    "walking slowly towards the side her toes point, caught mid-step, only her head turning \
     slightly towards the camera",
    "standing still for a moment with both feet planted, looking off down the street",
    "leaning lightly with one shoulder against the wall or railing beside her, the clothes \
     neither pressed flat nor hidden",
];

/// 기대는 자세(`POSES` 의 마지막)는 기댈 것이 있는 장소에서만 고른다.
fn has_something_to_lean_on(setting: &str) -> bool {
    [
        "wall",
        "railing",
        "door",
        "facade",
        "shutter",
        "column",
        "window",
        "shopfront",
        "balcony",
        "building",
        "arcade",
    ]
    .iter()
    .any(|w| setting.contains(w))
}

/// 자세를 고른다. 해시의 48~55비트를 쓴다 — 출신·헤어·장소·빛과 따로 움직인다.
fn pick_pose(setting: &str, hash: u64) -> &'static str {
    let n = if has_something_to_lean_on(setting) {
        POSES.len()
    } else {
        POSES.len() - 1
    };
    POSES[((hash >> 48) & 0xFF) as usize % n]
}

/// 무드별 장소(가중치 2)와 공통 장소(가중치 1)에서 착장 해시로 하나를 고른다.
/// 해시의 하위 16비트를 쓴다 — 헤어(16~31비트)·출신(상위 32비트)과 묶이지 않는다.
fn pick_setting(genre: StyleGenre, hash: u64) -> &'static str {
    let (own, _) = genre_setting(genre);
    let shared: Vec<&'static str> = SHARED_SETTINGS
        .iter()
        .copied()
        .filter(|s| !own.contains(s))
        .collect();
    let total = own.len() * 2 + shared.len();
    let slot = (hash & 0xFFFF) as usize % total;
    if slot < own.len() * 2 {
        own[slot / 2]
    } else {
        shared[slot - own.len() * 2]
    }
}

fn genre_setting(genre: StyleGenre) -> (&'static [&'static str], &'static str) {
    use StyleGenre as G;
    match genre {
        G::Minimal => (
            &[
                "sunlit stone steps with a long clean shadow across them",
                "a wide open plaza with pale paving",
                "a glass facade with a single clean line of reflection",
                "the bright exterior wall of a gallery with one doorway",
            ],
            "soft overcast light from one side",
        ),
        G::Romantic => (
            &[
                "beneath a curved balcony on an ivory building",
                "a bright terrace with a low rendered wall",
                "a street with dappled shade from the trees",
                "the ivory-painted exterior of a small cafe",
            ],
            "gentle warm light from the side, reaching only part of the background",
        ),
        G::ModernChic => (
            &[
                "the exterior of a contemporary art museum with a strong horizontal line",
                "a walkway of glass and brushed metal",
                "a bright limestone building with crisp joints",
                "an architectural staircase with sharp edges",
            ],
            "clear daylight raking from the side",
        ),
        G::Bohemian => (
            &[
                "a weathered terracotta plaster wall with dried plants nearby",
                "a street of pale stone houses with an old timber door",
                "a shaded courtyard with worn tiles and a large potted plant",
                "a bright stone building with deep window reveals",
                "a terrace with potted plants and a low rendered wall",
            ],
            "low late-afternoon sun from the side, long soft shadows",
        ),
        // 운동하는 곳이 아니라 운동복을 입고 하루를 보내는 곳. 트랙·운동장은 뺐다 —
        // 배경이 운동 장소로 굳으면 화보가 아니라 운동 사진이 된다.
        G::SportyCasual => (
            &[
                "a sunlit cafe terrace with a plain awning",
                "bright concrete steps outside a modern building",
                "a riverside walkway with open sky behind her",
                "the glass facade of a building reflecting soft daylight",
            ],
            "bright open daylight",
        ),
        // 어둡고 거친 골목 하나로 굳지 않도록 색·빛·공간감이 있는 거리를 섞는다.
        G::Street => (
            &[
                "a shopfront clad in coloured tiles on a busy street",
                "a red-brick street with shop signs receding behind her",
                "a sunlit square where light and shadow cross the paving",
                "pale concrete architecture with a strong geometric edge",
                "rough concrete and a metal roller shutter, a few small weathered posters",
            ],
            "clear side light with crisp shadows that gives the frame depth",
        ),
        G::Mannish => (
            &[
                "a terrace in front of a brick building",
                "a space where concrete meets warm timber",
                "the entrance of a bookshop",
                "a wide sunlit pavement",
            ],
            "warm afternoon light from the side",
        ),
        G::Classic => (
            &[
                "a stone entrance with a heavy timber door",
                "a tree-lined avenue",
                "in front of an old bookshop window",
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
            "soft overcast light with a clear direction, enough to show the weave of the fabric",
        ),
        G::OutdoorCasual => (
            &[
                "stone steps at a city trailhead with green foliage behind",
                "a park gate with a gravel path running past it",
                "a concrete underpass opening onto trees",
            ],
            "cool overcast light from one side",
        ),
        // 장소는 시간을 정하지 않는다 — 저녁인지 낮인지는 빛이 정한다(`pick_light`).
        // 장소에 "dusk" 를 박아 두면 공통 빛(한낮)과 짝이 맞지 않는 사진이 나온다.
        G::Glam => (
            &[
                "a city street of stone facades with warm shopfront light behind her",
                "the warmly lit entrance of a contemporary gallery",
                "a textured stone building with a terrace in front of it",
                "a covered arcade open to the street at one end",
            ],
            "the last warm light of early evening, low from the side, with a few lamps \
             already lit in the background",
        ),
        // 엣지: 갤러리 입구·질감 있는 상가 외벽·색이 있는 타일·아케이드·철제 보행교. 어두운
        // 골목 하나로 수렴하지 않게 밝은 장소와 색이 있는 장소를 섞는다. 장소는 시간을 정하지
        // 않는다 — 빛이 정한다(`pick_light`).
        G::Edgy => (
            &[
                "the entrance of a contemporary gallery with a raw concrete frame and one coloured door",
                "a shopping street of textured stone with dark-framed shop windows",
                "a covered city arcade with a patterned tiled floor",
                "a building corner clad in coloured ceramic tiles with a sharp edge",
                "a pedestrian bridge of painted steel with the city behind it",
            ],
            "crisp daylight from one side that sharpens the lines of the clothes",
        ),
        // 팝: 그래픽 타일의 카페 테라스·디자인 서점 입구·색 칠한 계단·갤러리 앞·줄무늬 차양.
        // 놀이공원·캔디숍·그래피티·네온으로 굳히지 않는다. 시간은 빛이 정한다(`pick_light`).
        G::Pop => (
            &[
                "a cafe terrace with graphic two-tone floor tiles",
                "the entrance of a design bookshop with a coloured door frame",
                "a city staircase painted in one bright colour",
                "a gallery forecourt with pale paving and one coloured wall",
                "a terrace under a striped awning in front of a pale rendered wall",
            ],
            "bright clear daylight that keeps the colours of the clothes true",
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

/// 룩북 모델의 출신. 착장 해시로 하나를 고르며, 칸 수가 비중이다(20칸 중 한국 10칸).
///
/// 프롬프트가 출신을 말하지 않으면 이미지 모델은 기본값으로 서양인을 그린다. 9월까지는
/// 장소 문장의 "Hangang riverside", "Seoul" 이 그 역할을 대신해 한국인이 나왔는데,
/// 2026-09-11 장르별 문장을 다시 쓰며 지명이 빠지자 아시아 모델이 사라졌다. 늘 한국인일
/// 필요는 없어서(2026-10-15 사용자 판단) 한국을 절반으로 두고 나머지를 섞는다.
///
/// 출신만 한 단어로 적고 생김새는 적지 않는다 — 5월의 "Korean idol face" 같은 외모
/// 묘사는 고정관념이자 인위적인 얼굴로 수렴하는 원인이었다.
const MODEL_ORIGINS: [&str; 20] = [
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Korean",
    "Japanese",
    "Japanese",
    "Chinese",
    "Southeast Asian",
    "South Asian",
    "European",
    "European",
    "Black",
    "Black",
    "Latina",
];

/// 착장 해시로 모델 출신을 고른다. 헤어(하위 자리)·장소와 함께 움직이지 않도록 상위
/// 32비트를 쓴다.
fn model_origin(hash: u64) -> &'static str {
    MODEL_ORIGINS[((hash >> 32) as usize) % MODEL_ORIGINS.len()]
}

/// 장르별 헤어스타일 후보와 가중치. 착장 해시로 하나를 고른다.
///
/// 예전에는 네 가지를 모든 장르가 함께 써서, 스트리트 코디에 단정한 로우 번이,
/// 클래식 코디에 부스스한 웨이브가 나왔다(2026-10-15 사용자 지적). 그렇다고 무드마다
/// 후보를 갈라 버리면 매니시는 늘 짧은 머리, 스트리트는 늘 튀는 머리가 되어 다시
/// 캐릭터처럼 보인다 — 얼굴 치수를 걷어내며 줄인 과장이 헤어로 돌아오는 셈이다.
///
/// 그래서 생머리·단발·로우 포니테일·느슨한 번 같은 일상 헤어를 여러 무드가 함께 쓰고
/// 높은 가중치를 준다. 무드는 같은 헤어의 정돈 정도(클래식의 단정한 번, 로맨틱의
/// 느슨한 번)와, 픽시·투 번·울프컷·양갈래 같은 개성 강한 헤어의 낮은 빈도로만
/// 드러낸다. 옷이 이미 무드를 전하므로 헤어까지 매번 같은 방향으로 밀 필요는 없다.
///
/// 모자·핀처럼 추천 목록에 없는 소품은 넣지 않는다 — 공통 회피 규칙의 "목록에 없는
/// 소품 금지" 와 부딪친다.
fn genre_hair(genre: StyleGenre) -> &'static [(&'static str, u32)] {
    use StyleGenre as G;
    match genre {
        G::Minimal => &[
            (
                "straight shoulder-length hair with a center part, tucked behind the ears",
                3,
            ),
            ("low ponytail, neat but not slicked down", 3),
            ("chin-length bob, simple and unstyled", 2),
            ("loose low bun", 2),
        ],
        G::Classic => &[
            ("neat low bun at the nape", 3),
            (
                "chin-length bob with the ends turned under slightly, restrained volume",
                3,
            ),
            (
                "shoulder-length soft waves, one side tucked behind the ear",
                2,
            ),
            ("neat low ponytail", 2),
        ],
        G::Romantic => &[
            ("long natural straight hair worn down, soft and unstyled", 3),
            ("soft loose waves falling past the shoulders", 3),
            ("half-up hair, loosely gathered", 2),
            ("loose low bun with face-framing strands", 2),
            ("loose low braid", 1),
        ],
        G::ModernChic => &[
            ("long straight hair with a side part, natural shine", 3),
            ("jaw-length bob, natural texture", 3),
            ("smooth low bun", 2),
            ("slicked-back hair", 1),
        ],
        G::Bohemian => &[
            ("long loose natural waves with a middle part", 4),
            ("shoulder-length air-dried hair", 2),
            ("undone low bun", 2),
            ("loose side braid with undone strands", 1),
        ],
        G::Street => &[
            ("long straight hair worn down", 3),
            ("chin-length bob, slightly tousled", 3),
            ("high ponytail", 2),
            ("loose low bun", 2),
            ("textured wolf cut", 1),
            ("half-up space buns", 1),
        ],
        G::Mannish => &[
            ("long straight hair tucked behind the ears", 3),
            ("loose low ponytail", 3),
            ("jaw-length bob with a side part", 2),
            ("slicked-back hair", 1),
            ("short pixie crop", 1),
        ],
        G::SportyCasual => &[
            ("high ponytail with a few loose strands", 4),
            ("high messy bun", 3),
            ("low ponytail", 2),
            ("two low braids", 1),
        ],
        // 정돈은 하되 볼륨과 컬은 키우지 않는다. 목선·어깨가 디테일인 옷이 많아서
        // 어깨 위로 흘러내리는 머리는 뒤로 넘기거나 묶는 쪽에 무게를 둔다.
        G::Glam => &[
            ("sleek straight hair tucked behind the ears", 3),
            ("polished soft waves brushed back off the shoulders", 3),
            ("smooth low bun", 2),
            ("jaw-length bob with a neat side part", 2),
            ("low ponytail, smooth at the crown", 1),
        ],
        // 엣지: 형태가 중심인 옷이라 목선·어깨를 덮지 않는 헤어에 무게를 둔다. 짧은 머리도
        // 낮은 비중으로만 둔다 — 늘 짧은 머리면 캐릭터가 된다.
        G::Edgy => &[
            ("sleek straight hair tucked behind the ears", 3),
            ("jaw-length blunt bob, natural texture", 3),
            ("low ponytail, smooth at the crown", 2),
            ("straight shoulder-length hair with a center part", 2),
            ("short textured crop", 1),
        ],
        // 팝: 일상 헤어 위주. 튀는 헤어로 무드를 대신하지 않는다.
        G::Pop => &[
            ("long straight hair worn down", 3),
            ("chin-length bob with a little movement", 3),
            ("high ponytail", 2),
            ("loose low bun", 2),
            ("half-up hair, loosely gathered", 1),
        ],
        // 남성 장르의 코디를 여성 모델로 그릴 때. 특정 무드 없이 일상적인 헤어로 둔다.
        _ => &[
            (
                "messy long waves with curtain bangs, effortless undone texture",
                1,
            ),
            ("chin-length blunt bob, slightly tousled", 1),
            (
                "low loose bun with face-framing strands, relaxed and casual",
                1,
            ),
            (
                "center-part shoulder-length hair, natural air-dried texture",
                1,
            ),
        ],
    }
}

/// 가중치대로 헤어를 고른다. 장소(해시의 하위 자리)·출신(상위 32비트)과 묶이지
/// 않도록 그 사이의 16~31비트를 쓴다.
fn pick_hair(genre: StyleGenre, hash: u64) -> &'static str {
    let choices = genre_hair(genre);
    let total: u32 = choices.iter().map(|(_, w)| w).sum();
    let mut slot = ((hash >> 16) & 0xFFFF) as u32 % total;
    for (hair, w) in choices {
        if slot < *w {
            return hair;
        }
        slot -= w;
    }
    choices[0].0
}

/// 글램의 메이크업 포인트. 눈 또는 입술 중 한 곳에만 둔다. 둘을 모델에 맡기면 두
/// 표현이 겹쳐 짙은 화장이 된다. 해시의 56번째 비트로 고른다 — 다른 선택이 쓰는 자리
/// (장소 0~15, 헤어 16~31, 빛 40~47, 자세 48~55)와 겹치지 않는다.
fn glam_makeup(hash: u64) -> &'static str {
    if (hash >> 56) & 1 == 0 {
        "a refined eye look — softly defined lashes and a little warm shadow — with bare, \
         natural lips"
    } else {
        "a clear but soft lip colour, with the eyes left nearly bare"
    }
}

/// 빛을 받는 소재가 목록에 있는가. 있으면 반사를 살리는 문장을, 없으면 무광을 지키는
/// 문장을 넣는다 — 글램이라고 무광 옷을 반짝이게 그리면 안 된다.
fn has_sheen_fabric(items: &str) -> bool {
    let lower = items.to_lowercase();
    [
        "satin", "metallic", "lurex", "velvet", "sequin", "lustrous", "sheen",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

fn build_image_prompt(genre: StyleGenre, items: &str, hash: u64) -> String {
    let hair = pick_hair(genre, hash);

    // 장소도 착장 해시로 고른다. 헤어(`pick_hair`)와 출신(`model_origin`)은 다른
    // 비트를 쓰므로, 장소는 하위 자리를 그대로 쓴다.
    let light = pick_light(genre, hash);
    let place = pick_setting(genre, hash);
    let pose = pick_pose(place, hash);
    let setting = format!(
        "{} — the background brings {}",
        place,
        genre_backdrop(genre)
    );

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
    // 얼굴은 수치가 아니라 캐스팅과 빛으로 올린다.
    //
    // 예전에 "very small head", "V-line face" 같은 치수를 적었더니 게임 캐릭터가
    // 나왔다. 그걸 걷어낸 뒤에는 질감만 요구하고 건강함을 요구하지 않아서, 이번엔
    // 피부가 칙칙하게 나왔다. 두 실패가 반대 방향이므로 양쪽을 다 적는다 —
    // 모공과 비대칭은 남기되, 윤기 없고 얼룩진 피부는 명시적으로 막는다.
    // 나이·메이크업·표정은 무드가 고른다. 글램은 "주목받는 것이 편한" 인물이라, 공통
    // 문장("not posed for anyone", "minimal everyday makeup")을 두고 반대 지시를 덧붙이면
    // 모델이 둘 중 하나를 무시한다. 그래서 그 자리만 갈아 끼운다. 다른 무드의 문장은
    // 예전과 한 글자도 같다 — 바뀌면 그 무드의 이미지 캐시가 통째로 어긋난다.
    let (age, makeup, expression) = match genre {
        StyleGenre::Glam => (
            "between her mid-twenties and mid-thirties",
            glam_makeup(hash),
            "She is comfortable being looked at: a relaxed, self-assured gaze and at most a \
             slight smile — present and at ease, never performing.",
        ),
        // 엣지: 편안하면서 자신감 있는 표정. 화장은 다른 무드처럼 가볍게 둔다 — 짙은 눈화장·
        // 어두운 입술로 무드를 만들지 않는다.
        StyleGenre::Edgy => (
            "in her twenties or early thirties",
            "minimal everyday makeup, natural lips",
            "Her expression is relaxed and self-possessed: a calm, steady look, confident \
             without performing.",
        ),
        // 팝: 밝고 편안한 자신감. 화장은 가볍게 — 옷 색에 맞춘 색조로 무드를 만들지 않는다.
        StyleGenre::Pop => (
            "in her twenties or early thirties",
            "minimal everyday makeup, natural lips",
            "Her expression is bright and relaxed — an easy, confident smile or a cheerful \
             glance, never a posed advertising grin.",
        ),
        _ => (
            "in her twenties",
            "minimal everyday makeup, natural lips",
            "Her expression is relaxed and spontaneous, not posed for anyone.",
        ),
    };
    let base_face = format!(
        "a real adult {} woman {age} with distinctive individual facial features \
         and natural facial asymmetry — a particular person rather than a generic type. \
         Realistic skin texture: visible pores, fine facial hair and baby hairs at the \
         hairline, small variations in skin tone, subtle imperfections. The skin is healthy \
         and well-rested — even in tone and warm in colour, never sallow, grey, blotchy or \
         tired — but it is never smoothed. Groomed natural brows, {makeup}, \
         realistic eyes with subtle reflections and a clear catchlight. \
         {expression} \
         Hairstyle: {}, individual strands and a few loose hairs visible.",
        model_origin(hash),
        hair
    );

    // 비율은 해부학을 조작하지 않고 자세로 만든다. 어깨가 열리고 목이 길면 같은
    // 체형이라도 머리가 커 보이지 않는다.
    let base_body = "realistic adult body proportions, natural head-to-body ratio, \
         believable shoulder width and waist, ordinary anatomical variation. \
         She carries herself with an easy upright posture — shoulders relaxed and open, \
         neck long, chin level — with natural weight distribution. Each leg runs straight from its own hip to its own \
         foot; the legs never cross over or swap sides.";

    // 자세는 `POSES` 다섯 중 착장 해시로 고른다(`pick_pose`).
    //
    // 한때 모든 arm 을 같은 보행 포즈로 통일했더니 서로 다른 코디를 같은 자리에 세워
    // 찍은 것처럼 보였고, 그다음엔 장르마다 자세를 하나씩 박아 두었더니 같은 장르는
    // 시선과 몸 각도까지 똑같아졌다. 지금은 장르와 상관없이 후보에서 고르고, 아래
    // 규칙만 공통으로 건다.
    let base_pose_rule = "Relaxed and unforced — she is not touching her hair or face, and \
         a calm, easy look at the camera is fine. Full body visible from head to shoes. \
         Her arms and bag never cover the main details of the top. No exaggerated twisting, \
         no strict profile or back view.";

    // 촬영 조건은 장르와 무관하게 같다. 인물을 묘사하기 전에 "어떤 카메라로 어떤
    // 상황에서 찍은 사진인지" 를 먼저 정해야 사진처럼 나온다. 그래서 카메라·구도·
    // 색·스타일을 각각 따로 적는다 — 한 문단에 뭉쳐 두면 한쪽만 고쳐지고 나머지가 남는다.

    // 렌즈와 카메라 높이. 두상이 커 보이는 건 대개 광각과 하이앵글 탓이다.
    let base_camera = "Shot on a full-frame mirrorless camera with an 85mm f/2 lens, \
         from roughly chest height. Full-length framing with natural lens compression and \
         a shallow depth of field — the background recognisable but softly out of focus. \
         Never shot from above: a high camera enlarges the head and shortens the legs. \
         Natural perspective and realistic optical characteristics, no wide-angle distortion.";

    // 사진을 찍고 있다는 상황 자체를 적어 준다. 이게 빠지면 카메라를 의식한
    // 스톡 사진 자세가 나온다.
    //
    // 글램은 카메라를 의식하지 않는 장면이 아니라, 의식하면서도 편안한 장면이다. 첫
    // 문장만 무드가 고르고 구도 조건은 같이 쓴다.
    let candid = match genre {
        StyleGenre::Glam => {
            "Candid editorial photography: she is out in the city, aware of the camera and \
             at ease with it, photographed from across the pavement."
        }
        StyleGenre::Edgy | StyleGenre::Pop => {
            "Candid editorial photography: she is out in the city on her own errand, aware of \
             the camera and unbothered by it, photographed from across the pavement."
        }
        _ => {
            "Candid editorial photography: she is going about her own business and is not \
             aware of the camera, photographed from across the pavement."
        }
    };
    let base_composition = format!(
        "{candid} \
         Slightly imperfect framing, off-centre composition with some negative space, \
         relaxed natural posture, and a believable spatial relationship between her and \
         the background. Full-length framing: her whole body is inside the frame with \
         visible ground beneath her feet and a clear margin of space below the shoes — \
         the bottom edge never cuts across her legs, ankles or feet."
    );

    // 색은 과하게 올리지 않는다. 채도를 눌러야 옷의 실제 색이 읽힌다.
    //
    // 글램은 예외다. 에메랄드 새틴·메탈릭 니트를 주면서 "채도를 누른 중립색 팔레트" 를
    // 요구하면 문장부터 옷과 부딪친다(2026-10-17 사용자 지적). 글램은 옷의 원래 색과
    // 채도를 지키고, 배경이 그 색을 받쳐 주게 한다. 다른 무드의 문장은 그대로다 —
    // 바뀌면 그 무드의 저장된 사진이 캐시에서 빗나간다.
    // 엣지도 코발트·버건디·올리브 같은 옷의 색을 누르지 않는다.
    let (saturation, palette, moment) = match genre {
        StyleGenre::Glam | StyleGenre::Edgy | StyleGenre::Pop => (
            "true-to-life saturation",
            "every garment keeps its own colour and saturation, and the colours of the \
             background support the clothes rather than compete with them",
            "photorealistic, natural",
        ),
        _ => (
            "restrained saturation",
            "a muted neutral palette and slightly desaturated tones",
            "photorealistic, understated",
        ),
    };
    let base_color = format!(
        "Natural cinematic colour grading with realistic skin tones, \
         subtle contrast, {saturation} and a fine film-like grain. \
         Balanced exposure with no blown highlights."
    );

    // 지향점. "평범한 일상 스냅" 이 아니라 "실제로 있을 법한, 감각적으로 촬영한 화보".
    let base_style = format!(
        "Contemporary fashion lookbook photography — the kind of premium \
         online editorial that shows how real clothes are actually worn. The styling reads \
         current, wearable and effortless rather than expensive or ceremonial. \
         An authentic candid moment, {moment}. \
         Choose a real location whose colours and materials suit the clothes, and let the \
         texture and light of the background carry the mood quietly. The frame as a whole \
         feels deliberately composed, with {palette}. \
         A real person photographed on location, not a digitally created fashion avatar."
    );

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

    // 옷·가방·신발도 얼굴만큼 중요한 피사체라고 선언한다.
    //
    // 아이템 이름만 넘기면 이미지 모델은 가방을 "가방" 수준으로 뭉갠다 — 가죽 결이
    // 플라스틱처럼 되고 신발 앞코가 흐려진다. 무엇을 입었는지는 {items} 가 정하므로
    // 여기서는 그것들을 "얼마나 또렷하게, 어떤 질감으로" 그릴지만 말한다.
    // 아이템 자체를 바꾸라는 지시가 아니므로 base_fidelity 와 충돌하지 않는다.
    let base_garments = "The clothing and the accessories deserve the same visual attention \
         as the face — they are subjects of this photograph, not background detail. \
         Fabric: realistic garment weight, visible textile weave, natural draping under \
         gravity, compression folds where the body bends, subtle wrinkles rather than a \
         perfectly smooth surface. \
         Bag: believable construction with correct straps and hardware, realistic leather \
         thickness, natural grain, precise stitching and clean edge finishing; metal that \
         reads as solid metal with restrained reflections, never mirror-bright. \
         Shoes: correct proportions and a clearly defined sole, visible stitching and \
         correct laces or closures, realistic creasing at the flex point. \
         Each piece holds its own shape and looks well-constructed rather than generic.";

    // 인물과 배경이 따로 놀지 않게 하는 조건. 빛의 방향·색온도·그림자가 어긋나면
    // 인물만 따로 렌더링해 합성한 것처럼 보인다.
    let base_light = "Soft directional light on her face with a gentle shadow falloff across \
         the cheek, realistic highlights on the skin, and a clear catchlight in the eyes — \
         never flat, shadowless light that leaves the face dull. \
         The subject and the background share one light: the same direction, the same \
         colour temperature, and shadows that agree with each other. \
         She was photographed standing in this place, not cut out and placed into it.";

    // 회피 목록에서 "ugly face", "big head", "ordinary pedestrian look" 을 뺐다.
    // 특히 "ordinary pedestrian look" 을 금지하면 현실에서 볼 법한 사람의 특징을
    // 모델이 의도적으로 지운다. 대신 실제로 걸러야 할 것 — 3D 렌더, 뷰티 필터,
    // 인위적인 신체 비율 — 을 명시한다.
    //
    // 화장·자세·광택을 막는 세 항목은 무드가 고른다. 글램에서 "excessive makeup" 과
    // "fashion-model posing" 을 그대로 두면 정돈된 포인트 메이크업과 편안한 눈맞춤까지
    // 막히고, "excessive gloss" 는 새틴의 자연스러운 반사까지 지운다. 그래서 글램은 막을
    // 것을 구체적으로 적는다 — 과장된 유혹 표정, 눈·입술을 함께 진하게 한 화장, 번들거리는
    // 피부, 하얗게 날아간 광택.
    let (makeup_avoid, posing_avoid, gloss_avoid) = match genre {
        StyleGenre::Glam => (
            "heavy contouring, strong eye makeup and strong lip colour together, glitter \
             makeup, false lashes",
            "exaggerated runway posing, pouting lips, sultry or seductive expression, fixed \
             advertising smile",
            "oily or shiny skin, blown-out highlights on satin or metallic fabric",
        ),
        // 엣지: 무광 가죽을 번들거리게 그리지 않는다. 화난 듯한 표정으로 무드를 만들지 않는다.
        StyleGenre::Edgy => (
            "excessive makeup, dark smoky eyes, dark lipstick",
            "fashion-model posing, scowling or aggressive expression",
            "excessive gloss, wet-look or high-shine leather",
        ),
        // 팝: 아이돌·아이 같은 포즈와 옷 색에 맞춘 화장으로 무드를 대신하지 않는다.
        StyleGenre::Pop => (
            "excessive makeup, glitter makeup, coloured eyeshadow matched to the clothes",
            "fashion-model posing, cutesy or childish poses, idol-style finger hearts",
            "excessive gloss",
        ),
        _ => (
            "excessive makeup",
            "fashion-model posing",
            "excessive gloss",
        ),
    };
    let base_avoid = format!(
        "male model, masculine face, masculine bone structure, \
         3D render, CGI, video game character, fashion game avatar, \
         anime face, doll-like face, porcelain skin, plastic skin, \
         beauty filter, excessive skin smoothing, oversized eyes, \
         impossibly small head, elongated limbs, artificial body proportions, \
         perfect facial symmetry, wax figure, mannequin, \
         distorted face, distorted mouth, awkward lip shape, \
         sallow or grey skin, blotchy or muddy complexion, tired dull face, \
         heavy under-eye shadows, flat shadowless light on the face, \
         high camera angle looking down at the subject, wide-angle facial distortion, \
         waxy face, unnatural HDR, oversharpening, {makeup_avoid}, \
         generic stock-photo smile, {posing_avoid}, perfectly posed stock photo, \
         catalog pose, ecommerce posture, stiff standing, symmetrical front pose, \
         cropped body, cropped legs, tight framing, oversaturated colors, harsh lighting, \
         hair clips, jewellery, props or accessories that are not part of the listed outfit, \
         balls, rackets, yoga mats, water bottles, gym or sports equipment of any kind, \
         heavy colour grading, a single colour washed over the whole frame, \
         studio backdrop, obvious set dressing, subject lit differently from the background, \
         cheap-looking materials, plastic-looking leather, low-detail fabrics, \
         blurry or poorly defined bag, mushy undefined shoes, cropped or hidden footwear, \
         generic fashion details, flat untextured garments, \
         waxy leather, fake metallic reflections, {gloss_avoid}, \
         melted or deformed shoe shapes, incorrect shoelaces, warped accessories, \
         floating straps, impossible garment seams, overly perfect fabric surfaces, \
         luxury-campaign stiffness, ceremonial posing, clothes that look unworn, \
         real brand logos, band names or album artwork, recognisable trademarks, \
         readable text on garments"
    );

    match genre {
        StyleGenre::Minimal => format!(
            r#"Fashion editorial photograph of a woman in her late 20s, dressed in a quiet, understated way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with the decoration stripped out: simple, unbroken silhouettes, a narrow range of colours across the whole look, no visible pattern or branding. Interest comes from the cut and the surface of the cloth, nothing added. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, logos, bold patterns, streetwear elements, sporty pieces, romantic frills, oversaturated colors."#
        ),
        StyleGenre::Romantic => format!(
            r#"Fashion editorial photograph of a woman in her early 20s, dressed in a soft, romantic way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn to let the soft things move: fluid fabrics that fall and gather, curved lines rather than straight ones, and any ruffle, lace or gathering in the pieces allowed to show. Light in colour and light in weight. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, masculine styling, dark heavy tones, oversized baggy fit, street edge, sporty elements."#
        ),
        StyleGenre::ModernChic => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s, dressed in a sharp, tailored way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with a sharp, deliberate line: a crisp silhouette with a clear edge, an asymmetric balance rather than a symmetrical one, and a strong contrast between light and dark across the look. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, casual sneakers, oversized baggy fit, vintage distressing, romantic frills, sporty elements."#
        ),
        // 글램: 빛을 받는 소재나 조형적인 선 하나가 코디를 이끌고, 인물은 그 옷을 입고
        // 주목받는 것을 편하게 즐긴다. 무대 의상·클럽·인형 같은 얼굴로 가지 않는다.
        // 나이·메이크업·표정·구도 첫 문장·회피 세 항목은 위에서 무드별로 골랐다.
        StyleGenre::Glam => {
            let surface = if has_sheen_fabric(items) {
                "Let the light model each listed fabric as it really is: satin, velvet and \
                 metallic pieces show soft, natural reflections that follow the drape and \
                 the folds, with their colour and detail still readable — never blown out \
                 to white. Matte pieces stay matte."
            } else {
                "Every listed fabric keeps its own surface: matte fabrics stay matte, with \
                 no sheen added."
            };
            format!(
                r#"Fashion editorial photograph of a woman between her mid-twenties and mid-thirties, dressed to be noticed and at ease with it. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn so that one or two pieces carry the look, through a fabric that catches the light or a sculpted line, while the plainer pieces hold it steady. Nothing is shortened, opened up or embellished: every neckline, hem, slit and surface stays exactly as listed. Her hair stays clear of the neckline and the shoulder line. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {surface} Her skin keeps a natural finish, never oily or plastic. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, nightclub interior, neon lighting, stage costume, red-carpet styling, added sequins, crystals or embellishment that the listed garments do not have, a hemline shorter or a neckline lower than listed, necklaces, earrings, bracelets, sunglasses, scarves, bandanas, champagne glasses, doll-like glamour face."#
            )
        }
        // 엣지: 비대칭·각진 어깨·다리의 재단 같은 형태와, 가죽과 니트·울·데님의 표면 대비가
        // 보이게 입는다. 펑크 의상·장식·찢김·피어싱·타투를 더하지 않는다.
        StyleGenre::Edgy => format!(
            r#"Fashion editorial photograph of a woman {age}, dressed with a sharp, confident edge. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn so the shape of each piece reads: asymmetric lines, structured shoulders and the cut of the legs stay exactly as listed, and where leather sits against knit, wool or denim the contrast between the surfaces shows. Nothing is shortened, opened up, ripped or embellished: every neckline, hem and surface stays exactly as listed. Her hair stays clear of the neckline and the shoulder line. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. Every listed fabric keeps its own surface: matte leather stays matte, smooth leather shows only its natural soft sheen, knit and denim keep their texture. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, punk costume, studs, spikes, chains, safety pins, rips, distressing or hardware that the listed garments do not have, piercings, tattoos, chokers, necklaces, earrings, sunglasses, a dark grungy alley, graffiti, neon lighting, nightclub interior."#
        ),
        // 팝: 색과 무늬가 착장을 이끈다. 목록의 색·배색·스트라이프·체크·그래픽을 그대로 두고 새
        // 프린트·로고·장식·소품을 더하지 않는다. 어린아이 같은 연출·코스튬으로 가지 않는다.
        StyleGenre::Pop => format!(
            r#"Fashion editorial photograph of a woman {age}, dressed in a bright, playful, colour-led way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn so the colours and patterns of the pieces read clearly: every listed colour, colour-block, stripe, check and graphic stays exactly as listed, with no new print, logo, slogan or decoration added. Bright pieces sit together just as they are listed. A piece described as worn closed stays fastened all the way — never open it to show the colour of what is underneath. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, an amusement park, a candy shop, a graffiti wall, neon lighting, nightclub interior, costume or cosplay styling, childish or kid-like styling, toy-like props, balloons, idol stage styling, jewellery, glasses, bandanas, hats or scarves that are not listed, added prints or logos."#
        ),
        StyleGenre::Bohemian => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in a loose, bohemian way. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn loose and layered: natural fibres with visible texture, layers that overlap without being tucked or fastened, and any pattern in the pieces left to read. Warm, earthy colour rather than clean brights. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, minimal clean styling, corporate look, sporty elements, neon colors, tech fabrics."#
        ),
        // 이 프롬프트의 내용(레깅스·스포츠브라·바이커숏·애슬레저)은 원래부터
        // 스포티 캐주얼이었다. 예전 분류가 "모델 사복 + 애슬레저"를 한 장르로
        // 묶고 있어서 off_duty 에 붙어 있었을 뿐이라, 문구만 장르에 맞춘다.
        StyleGenre::SportyCasual => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in sportswear worn as daily clothes. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn for movement: technical and knitted performance fabrics, elasticated or drawcord waists and cuffs, a silhouette built for activity. Function visible in the construction rather than decoration. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, formal styling, vintage distressing, dark moody tones, heavy makeup, aggressive gym energy, harsh lighting."#
        ),
        // 신규 장르. 애슬레저를 스포티 캐주얼로 분리하면서 비게 된 자리를 채운다.
        // 데님·가죽재킷·티셔츠 같은 기본 아이템에 힘을 뺀 구성이 이 장르의 정의다.
        StyleGenre::Street => format!(
            r#"Fashion editorial photograph of a woman in her late teens or early 20s, dressed in streetwear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn oversized and off-balance: volume through the shoulders and legs, one piece clearly larger than the body, any graphic or print left prominent. Sturdy fabrics like denim and cotton twill, nothing delicate. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, feminine soft styling, luxury campaign mood, romantic atmosphere, pastel tones."#
        ),
        StyleGenre::Mannish => format!(
            r#"Fashion editorial photograph of a woman in her 20s, dressed in menswear-leaning pieces. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn like borrowed tailoring: structured shoulders that sit wide, straight or wide legs breaking low, fabrics with a dry matte finish. Cut for a man's proportions and worn by her without adjustment. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, feminine delicate styling, formal look, luxury campaign mood."#
        ),
        StyleGenre::Classic => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s on her way to work. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn with a tidy, correct fit: collars sitting flat, sleeves and hems at the right length, outer layers worn open and hanging straight. Nothing oversized and nothing clinging — the proportions are the conventional ones. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, gym clothing, distressed vintage, heavy streetwear, evening glamour, romantic frills."#
        ),
        StyleGenre::Preppy => format!(
            r#"Fashion editorial photograph of a woman in her early 20s, dressed in preppy pieces. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn tidily and brightly: neat collars, layers sitting flat, classic proportions, and any stripe or check left to read clearly. Collegiate and upbeat rather than formal. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, streetwear, athletic gear, distressed denim, evening wear, bohemian layering."#
        ),
        StyleGenre::Workwear => format!(
            r#"Fashion editorial photograph of a woman in her mid 20s, dressed in workwear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn like clothes that get used: heavy, substantial cloth, sleeves pushed back, honest fading at the seams and cuffs, hardware that has seen work. Never decorative distressing. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, delicate fabrics, tailored formalwear, athletic gear, romantic frills, glossy luxury styling."#
        ),
        StyleGenre::OutdoorCasual => format!(
            r#"Fashion editorial photograph of a woman in her 20s in the city, dressed in outdoor gear. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn as real equipment: technical shells and insulating layers, functional details visible, drawcords and straps actually adjusted, layered for weather rather than for looks. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

Avoid: {base_avoid}, leggings, sports bra, gym styling, tailored formalwear, romantic frills, luxury campaign gloss."#
        ),
        StyleGenre::Amekaji => format!(
            // 남성/공용 기본 — 기존 힙스터 스타일
            r#"Fashion editorial photograph of a woman in her 20s, dressed in relaxed American casual. She must be female.

Face: {base_face}

Body: {base_body}

Outfit: The female model is wearing {items} — worn in rather than new: soft washes, natural fading at the seams and edges, a relaxed silhouette. Honest cotton and denim, nothing pristine. {base_fidelity}

Garments: {base_garments}

Pose: {pose}. {base_pose_rule}

Setting: {setting}. The background carries one material or architectural element that suits the colour and texture of the clothes, and never draws the eye away from them. It is there to show the clothes: frame her so their colour and fabric read clearly against it, in the same light as the background.

Lighting: {light}. {base_light}

Camera: {base_camera}

Composition: {base_composition}

Colour: {base_color}

Style: {base_style}

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
    searched_category: Option<&str>,
    clothes: &[Clothing],
    ctx: &OutfitContext<'_>,
    embedding: &std::sync::Arc<crate::services::embedding::EmbeddingService>,
) -> (String, Vec<ChatItem>, Vec<String>) {
    // 카테고리는 대화 모델이 search_wardrobe 에 넘긴 값을 먼저 쓴다. 이름에서 추정하는
    // 것은 옷장에 같은 이름이 있을 때만 맞는다 — 사용자가 말한 옷이 옷장에 없으면
    // 추정이 비어서 전 카테고리를 검색하게 되고, 그러면 "화이트 테니스 스커트" 가
    // 색만 같은 하이탑 스니커즈(0.70)에 걸린다.
    let cat_hint_str = searched_category
        .map(str::to_string)
        .or_else(|| extract_category_from_wardrobe(anchor_name, clothes));
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
            // 카테고리를 알면 그 안의 1위를 그대로 쓴다. 기준값으로 거르면 맞는 옷이
            // 버려진다 — "검정 더비" → 블랙 레더 더비 슈즈가 0.45, "가죽 라이더" → 블랙
            // 레더 라이더 자켓이 0.38 이다. 버려지면 아래에서 옷장 첫 옷으로 가는데,
            // 같은 카테고리의 가장 비슷한 옷이 그보다 항상 낫다.
            let min_similarity = if cat_hint.is_some() {
                f32::MIN
            } else {
                WARDROBE_MATCH_MIN_SIMILARITY
            };
            embedding
                .search_wardrobe(anchor_name, &filtered, 1)
                .await
                .ok()
                .and_then(|m| m.into_iter().next())
                .filter(|m| m.similarity > min_similarity)
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
            // DB에 유사 아이템도 없으면 같은 카테고리의 첫 아이템, 그것도 없으면 첫 아이템
            let same_category = cat_hint.and_then(|cat| clothes.iter().find(|c| c.category == cat));
            match same_category.or_else(|| clothes.first()) {
                Some(c) => c,
                None => {
                    return (
                        json!({"error": "wardrobe empty"}).to_string(),
                        Vec::new(),
                        Vec::new(),
                    );
                }
            }
        }
    };

    let result = build_final_outfit(scoring_anchor, clothes, ctx);
    match result {
        Some((_desc, mut items, chosen_ids)) => {
            // anchor 슬롯을 유저 원문으로 교체 (DB에 없어도 원문 유지)
            let anchor_slot = display_anchor_cat;
            let slot_key = slot_for_category(anchor_slot).unwrap_or("shoes");

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
                    wear: None,
                });
            }

            let desc = items
                .iter()
                .map(|i| format!("{}: {}", i.slot, i.name))
                .collect::<Vec<_>>()
                .join("\n");
            apply_wear(&mut items, clothes, ctx.temperature);
            let items_json: Vec<serde_json::Value> = items.iter().map(|i| {
                json!({"slot": i.slot, "name": i.name, "category": i.category, "owned": i.owned, "wear": i.wear})
            }).collect();
            let response = json!({ "outfit": desc, "items": items_json });
            (response.to_string(), items, chosen_ids)
        }
        None => (
            json!({"error": "no suitable outfit found"}).to_string(),
            Vec::new(),
            Vec::new(),
        ),
    }
}

/// 앞 코디에서 한 카테고리만 바꾼다.
///
/// 유저가 원하는 것을 말했으면 그 카테고리 안에서 가장 비슷한 옷을, 아니면 남은
/// 아이템들과의 궁합 점수 합이 가장 높은 옷을 고른다(팝은 `best_fit_swap` 참고). 지금 입은
/// 것은 후보에서 뺀다 — 바꿔 달라는 요청에 같은 옷이 돌아오면 안 된다.
#[allow(clippy::too_many_arguments)]
async fn tool_swap_item(
    base: &[ChatItem],
    category: &str,
    query: &str,
    clothes: &[Clothing],
    wide_clothes: &[Clothing],
    embedding: &std::sync::Arc<crate::services::embedding::EmbeddingService>,
    mood: Option<StyleGenre>,
    temperature: Option<f64>,
) -> Option<(Vec<ChatItem>, Vec<String>)> {
    let slot = slot_for_category(category)?;
    let current = base
        .iter()
        .find(|i| i.slot == slot)
        .map(|i| i.name.as_str());
    let pool = |from: &[Clothing]| -> Vec<Clothing> {
        from.iter()
            .filter(|c| c.category == category && Some(c.name.as_str()) != current)
            .cloned()
            .collect()
    };
    let mut candidates = pool(clothes);

    let kept: Vec<&Clothing> = base
        .iter()
        .filter(|i| i.slot != slot)
        .filter_map(|i| clothes.iter().find(|c| c.name == i.name))
        .collect();

    // 유저가 색을 말했으면 그 색 계열 이름만 후보로 둔다. 임베딩 검색은 무조건 1등을
    // 돌려주므로, 색이 없는 후보 중 1등(흰 스니커)이 "카멜색" 의 답이 돼 버린다.
    // 종류("로퍼", "부츠")도 같다 — 무드 안에 로퍼가 없으면 흰 스니커가 1등으로 온다.
    let colors = color_family(query);
    let kinds = kind_family(query);
    if !colors.is_empty() || !kinds.is_empty() {
        let fits = |c: &Clothing| {
            (colors.is_empty() || colors.iter().any(|w| c.name.contains(w)))
                && (kinds.is_empty() || kinds.iter().any(|w| c.name.contains(w)))
        };
        let by_words =
            |v: Vec<Clothing>| -> Vec<Clothing> { v.into_iter().filter(|c| fits(c)).collect() };
        candidates = by_words(candidates);
        if candidates.is_empty() {
            candidates = by_words(pool(wide_clothes));
        }
        if candidates.is_empty() {
            return None;
        }
    }

    let picked = if query.is_empty() {
        None
    } else {
        let found = embedding
            .search_wardrobe(query, &candidates, 1)
            .await
            .ok()
            .and_then(|m| m.into_iter().next())
            .and_then(|m| candidates.iter().find(|c| c.name == m.name));
        // 색이나 종류를 말했으면 이미 걸렀으므로 검색이 실패해도 첫 후보가 답이다.
        found.or_else(|| (!colors.is_empty() || !kinds.is_empty()).then(|| &candidates[0]))
    };
    let picked = picked.or_else(|| best_fit_swap(&candidates, &kept, mood, temperature))?;

    let new_item = ChatItem::from_clothing(picked, true)?;
    // 자리는 그대로 두고 그 칸만 바꾼다 — 화면과 이미지 문자열의 순서가 유지된다.
    let mut items = base.to_vec();
    match items.iter_mut().find(|i| i.slot == slot) {
        Some(item) => *item = new_item,
        None => items.push(new_item),
    }
    let ids = items
        .iter()
        .filter(|i| i.owned)
        .filter_map(|i| {
            clothes
                .iter()
                .chain(wide_clothes)
                .find(|c| c.name == i.name)
                .map(|c| c.id.clone())
        })
        .collect();
    tracing::info!(category, query, picked = %picked.name, "swap_item");
    Some((items, ids))
}

/// 유저가 원하는 것을 말하지 않은 바꾸기: 남은 옷과의 궁합 점수 합이 가장 높은 옷.
///
/// 팝이면 바꾼 뒤에도 팝으로 읽히는 옷을 먼저 본다 — 궁합 점수만 보면 기본색 옷이 이겨 색
/// 조합이 깨졌다(2026-10-17 실측: 화이트 티 + 레드 트러커 + 켈리그린 팬츠의 하의가 화이트
/// 데님으로). 그런 옷이 없으면 원래대로다. 팝의 온화한 날 겨울 외투는 홈처럼 후보가 아니다
/// (`mood_gate::hidden_on_mild_pop_day`). 다른 무드는 예전과 같다.
fn best_fit_swap<'a>(
    candidates: &'a [Clothing],
    kept: &[&Clothing],
    mood: Option<StyleGenre>,
    temperature: Option<f64>,
) -> Option<&'a Clothing> {
    let candidates: Vec<&Clothing> = candidates
        .iter()
        .filter(|c| !crate::services::mood_gate::hidden_on_mild_pop_day(c, mood, temperature))
        .collect();
    let keeps_pop: Vec<&Clothing> = if mood == Some(StyleGenre::Pop) {
        candidates
            .iter()
            .copied()
            .filter(|c| {
                let mut outfit = kept.to_vec();
                outfit.push(c);
                crate::services::mood_gate::reads_as_at(StyleGenre::Pop, &outfit, temperature)
                    == Some(true)
            })
            .collect()
    } else {
        Vec::new()
    };
    let pool: Vec<&Clothing> = if keeps_pop.is_empty() {
        candidates
    } else {
        keeps_pop
    };
    pool.into_iter().max_by_key(|c| {
        kept.iter()
            .map(|k| outfit_scorer::complement_score(k, c))
            .sum::<i32>()
    })
}

/// 유저가 말한 색 표현을 옷장 이름에 쓰는 색 단어들로 넓힌다. 색을 말하지 않았으면 빈 목록.
///
/// 옷장 이름은 "탄 스웨이드 로퍼", "다크브라운 스웨이드 스니커" 처럼 영어 색 이름을
/// 한글로 쓴다. 유저는 "카멜", "갈색", "검은" 처럼 말한다.
/// 이번 요청에서 이 카테고리를 바꿔도 되는지.
///
/// 유저가 카테고리를 말했으면 그것만. 말하지 않았으면 평가 전까지는 어느 칸이든,
/// 평가 뒤에는 이미 바꾼 칸만 — 평가가 아쉬운 점을 말하면 모델이 묻지도 않은 칸을 바꾼다.
fn swap_allowed(category: &str, asked: &[&str], swapped: &[String], evaluated: bool) -> bool {
    if !asked.is_empty() {
        return asked.contains(&category);
    }
    !evaluated || swapped.iter().any(|c| c == category)
}

/// 유저 메시지에 나온 카테고리. 아이템 이름에 흔히 쓰는 말로 찾는다.
fn mentioned_categories(message: &str) -> Vec<&'static str> {
    const WORDS: &[(&str, &[&str])] = &[
        (
            "상의",
            &[
                "상의",
                "티셔츠",
                "반팔",
                "긴팔",
                "셔츠",
                "니트",
                "맨투맨",
                "스웨트셔츠",
                "블라우스",
                "후드",
                "폴로",
                "탑",
            ],
        ),
        (
            "하의",
            &[
                "하의",
                "바지",
                "팬츠",
                "스커트",
                "치마",
                "슬랙스",
                "청바지",
                "레깅스",
                "조거",
            ],
        ),
        (
            "아우터",
            &[
                "아우터",
                "자켓",
                "재킷",
                "코트",
                "점퍼",
                "블루종",
                "가디건",
                "윈드브레이커",
                "패딩",
                "트렌치",
                "베스트",
            ],
        ),
        (
            "신발",
            &[
                "신발",
                "스니커",
                "로퍼",
                "부츠",
                "샌들",
                "구두",
                "힐",
                "플랫",
                "슈즈",
                "뮬",
            ],
        ),
        ("가방", &["가방", "백", "토트", "크로스", "숄더", "클러치"]),
    ];
    // "셔츠" 는 "스웨트셔츠"·"티셔츠" 안에도 있지만 모두 상의라 문제없다. "후드" 는 후드집업
    // (아우터)일 수도 있으나, 상담에서 후드라고 하면 대개 후드티다.
    WORDS
        .iter()
        .filter(|(_, words)| words.iter().any(|w| message.contains(w)))
        .map(|(cat, _)| *cat)
        .collect()
}

/// 옷장의 다른 옷으로 바꾸는 게 아니라 옷 자체를 고쳐 달라는 말인지.
///
/// 무늬·그래픽 같은 말과 "추가·넣어" 같은 말이 함께 있을 때만 본다. "로고 티로 바꿔줘" 는
/// 로고가 있는 다른 옷으로 바꾸라는 말이라 걸리지 않는다.
fn asks_to_modify_item(message: &str) -> bool {
    const DETAILS: &[&str] = &[
        "그래픽",
        "프린트",
        "자수",
        "로고",
        "패치",
        "레터링",
        "캐릭터",
        "무늬",
        "그림",
    ];
    const VERBS: &[&str] = &[
        "추가", "넣어", "넣고", "그려", "새겨", "박아", "달아", "붙여",
    ];
    DETAILS.iter().any(|d| message.contains(d)) && VERBS.iter().any(|v| message.contains(v))
}

/// 유저가 말한 아이템 종류를 옷장 이름에 쓰는 말로 넓힌다. 종류를 말하지 않았으면 빈 목록.
fn kind_family(query: &str) -> Vec<&'static str> {
    const FAMILIES: &[(&[&str], &[&str])] = &[
        (&["로퍼"], &["로퍼"]),
        (&["부츠"], &["부츠"]),
        (&["샌들"], &["샌들"]),
        (&["스니커", "운동화"], &["스니커"]),
        (&["힐"], &["힐"]),
        (&["플랫"], &["플랫"]),
        (&["뮬"], &["뮬"]),
        (&["구두"], &["로퍼", "더비", "옥스퍼드", "힐", "메리제인"]),
        (&["토트"], &["토트"]),
        (&["크로스"], &["크로스"]),
        (&["숄더"], &["숄더"]),
        (&["백팩"], &["백팩"]),
        (&["버킷"], &["버킷"]),
        (&["니트"], &["니트"]),
        (&["블라우스"], &["블라우스"]),
        (&["맨투맨"], &["맨투맨", "스웨트셔츠"]),
        (&["후드"], &["후드"]),
        (&["슬랙스"], &["슬랙스"]),
        (&["청바지", "데님"], &["데님", "진"]),
        (&["스커트", "치마"], &["스커트"]),
        (&["코트"], &["코트"]),
        (&["트렌치"], &["트렌치"]),
        (&["블레이저"], &["블레이저"]),
        (&["가디건"], &["가디건"]),
    ];
    let mut out: Vec<&'static str> = Vec::new();
    for (said, names) in FAMILIES {
        if said.iter().any(|w| query.contains(w)) {
            for n in names.iter() {
                if !out.contains(n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

fn color_family(query: &str) -> Vec<&'static str> {
    const FAMILIES: &[(&[&str], &[&str])] = &[
        (&["카멜", "탄", "코냑"], &["카멜", "탄 ", "코냑"]),
        (
            &["브라운", "갈색"],
            &["브라운", "카멜", "탄 ", "코냑", "초콜릿", "모카"],
        ),
        (&["블랙", "검은", "검정", "까만"], &["블랙"]),
        (&["화이트", "흰", "하얀"], &["화이트"]),
        (
            &["아이보리", "크림", "에크루"],
            &["아이보리", "크림", "에크루"],
        ),
        (&["베이지"], &["베이지"]),
        (&["네이비", "남색"], &["네이비"]),
        (&["그레이", "회색"], &["그레이", "차콜"]),
        (&["차콜"], &["차콜"]),
        (&["레드", "빨간", "빨강"], &["레드", "버건디"]),
        (&["버건디", "와인"], &["버건디", "와인"]),
        (&["올리브", "카키"], &["올리브", "카키"]),
        (&["실버", "은색"], &["실버"]),
        (&["골드", "금색"], &["골드"]),
        (&["핑크", "분홍"], &["핑크"]),
        (&["블루", "파란", "하늘"], &["블루", "스카이"]),
        (&["그린", "초록"], &["그린", "민트"]),
    ];
    let mut out: Vec<&'static str> = Vec::new();
    for (said, names) in FAMILIES {
        if said.iter().any(|w| query.contains(w)) {
            for n in names.iter() {
                if !out.contains(n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

fn tool_evaluate_outfit(
    names: &[String],
    clothes: &[Clothing],
    _user: Option<&crate::models::user_profile::UserStyleProfile>,
    mood: Option<StyleGenre>,
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

    // 글램·엣지에서 어두운 색이 겹쳐도 옷의 소재·형태로 대비가 있으면 문제로 보지 않는다 —
    // 홈의 무드 규칙(`mood_rules::dark_look_reads_for`)과 같은 판단이다. 이걸 문제로
    // 돌려주면 상담 모델이 get_outfit 을 다시 불러 기본 코디로 되돌아갔다.
    let garments: Vec<&Clothing> = items
        .iter()
        .copied()
        .filter(|c| matches!(c.category.as_str(), "상의" | "하의" | "아우터"))
        .collect();
    let dark_ok = crate::services::mood_rules::dark_look_reads_for(mood, &garments);

    // 같은 색상군 3+
    let mut cg_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for i in &items {
        let cg = outfit_scorer::color_group(i.color.as_deref().unwrap_or(""));
        if cg != "other" {
            *cg_counts.entry(cg).or_insert(0) += 1;
        }
    }
    for (cg, count) in &cg_counts {
        if *count >= 3 && !(dark_ok && *cg == "dark") {
            issues.push(format!("color_repetition: {} 색상이 {}개 반복", cg, count));
        }
    }

    // 전부 어두움
    let dark_count = items.iter().filter(|i| i.tone == Some(Tone::Dark)).count();
    if dark_count >= 3 && !dark_ok {
        issues.push("too_dark: 어두운 톤이 3개 이상".to_string());
    }

    // floating
    // 값이 없는 옷은 평균에서 뺀다(`outfit_scorer::known_mean`). 여성 옷은 이 값들이 전부
    // 비어 있어, 0 으로 세면 모든 코디가 "밋밋함"·"접지감 부족" 으로 나왔다.
    let avg_float = outfit_scorer::known_mean(items.iter().map(|i| i.floating_score));
    if avg_float.is_some_and(|f| f >= 5.0) {
        issues.push("floating_balance: 전체적으로 떠보임".to_string());
    }

    // texture 단조
    let avg_tex = outfit_scorer::known_mean(items.iter().map(|i| i.texture_depth_v2));
    if avg_tex.is_some_and(|t| t < 2.5) {
        issues.push("too_flat: 질감이 너무 밋밋함".to_string());
    }

    // grounding 부족 (신발+가방)
    let grounding_known: Vec<i32> = items
        .iter()
        .filter(|i| i.category == "신발" || i.category == "가방")
        .filter_map(|i| i.grounding_score)
        .map(i32::from)
        .collect();
    let grounding: Option<i32> =
        (!grounding_known.is_empty()).then(|| grounding_known.iter().sum());
    if grounding.is_some_and(|g| g <= 3) {
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

/// 조합을 고를 때 쓰는 주변 정보.
///
/// 체형·날씨·피드백·최근 이력을 인자로 하나씩 넘기면 호출부가 길어지고 순서를
/// 틀리기 쉬워서 묶어 둔다.
struct OutfitContext<'a> {
    user: Option<&'a crate::models::user_profile::UserStyleProfile>,
    temperature: Option<f64>,
    feedback: &'a outfit_scorer::FeedbackContext,
    recent: &'a outfit_scorer::RecentHistory,
    /// 고른 무드. 무드를 읽을 요소가 있는 조합을 먼저 고르는 데 쓴다(`mood_gate`).
    mood: Option<StyleGenre>,
}

/// 기준 아이템 없이 오늘의 코디를 만든다(`get_outfit_for_today`).
///
/// 기준 옷은 오늘 날씨에 맞는 옷(상의·하의·아우터) 가운데 고른다. 무드 요소를 판단하는
/// 무드(글램·엣지)면 무드 요소가 있는 옷을 먼저 본다. 같은 옷만 기준이 되지 않게 최근
/// 추천에 덜 나온 옷부터 시도한다. 조합·날씨 판정·무드 게이트는 `build_final_outfit` 이
/// 그대로 맡는다 — 기준 아이템이 있는 상담과 같은 경로다.
fn today_outfit(
    clothes: &[Clothing],
    ctx: &OutfitContext<'_>,
) -> (String, Vec<ChatItem>, Vec<String>) {
    // 홈과 같은 기준: 팝의 온화한 날에는 겨울 외투를 기준 옷·조합 후보로 쓰지 않는다. 기온을
    // 모르면 빼지 않는다(`mood_gate::hidden_on_mild_pop_day`).
    let usable: Vec<Clothing> = clothes
        .iter()
        .filter(|c| {
            !crate::services::mood_gate::hidden_on_mild_pop_day(c, ctx.mood, ctx.temperature)
        })
        .cloned()
        .collect();
    let clothes = usable.as_slice();
    let temp = ctx.temperature.unwrap_or(20.0);
    let mut anchors: Vec<&Clothing> = clothes
        .iter()
        .filter(|c| matches!(c.category.as_str(), "상의" | "하의" | "아우터"))
        .filter(|c| is_weather_appropriate(c, temp))
        .collect();
    if let Some(m) = ctx.mood.filter(|m| crate::services::mood_gate::judges(*m)) {
        let pieces: Vec<&Clothing> = anchors
            .iter()
            .copied()
            .filter(|c| crate::services::mood_gate::is_mood_piece(m, c) == Some(true))
            .collect();
        if !pieces.is_empty() {
            anchors = pieces;
        }
    }
    // 최근에 덜 나온 옷이 먼저다. 같으면 엣지에서는 보이는 색이 있는 옷(유채색 아우터, 또는
    // 코트를 여미지 않는 기온의 유채색 상의)을 먼저 시도한다 — 예전에는 이름순이라 늘
    // '다크브라운…' 시어링 재킷이 기준이 됐다(2026-10-17 상담 실측). 밝은색을 싫어한다고
    // 적은 사용자에게는 색을 앞세우지 않는다.
    let prefer_colour =
        ctx.mood == Some(StyleGenre::Edgy) && !ctx.user.is_some_and(|u| u.dislikes_bright_colors);
    // 코트를 여미지 않는 기온(12°C 이상)에서는 두꺼운 아우터를 색 때문에 앞세우지 않는다 —
    // 19.6°C 에 보온 안감 코발트 코트가 기준이 됐다(2026-10-17 실측). 두꺼운 옷의 날씨
    // 필터는 그대로다(`is_weather_appropriate`) — 색 우선만 좁힌다.
    let mild = temp >= crate::services::serving_ranker::CLOSED_COAT_BELOW_C;
    let shows_colour = |c: &Clothing| {
        prefer_colour
            && crate::services::recommendation_service::is_chromatic(c)
            && ((c.category == "아우터"
                && !(mild && c.thickness == crate::models::style_vocab::Thickness::Thick))
                || (c.category == "상의" && mild))
    };
    anchors.sort_by(|a, b| {
        let freq = |c: &Clothing| ctx.recent.item_freq.get(&c.id).copied().unwrap_or(0);
        freq(a)
            .cmp(&freq(b))
            .then_with(|| shows_colour(b).cmp(&shows_colour(a)))
            .then_with(|| a.name.cmp(&b.name))
    });
    // 기준 옷은 코디에 반드시 들어간다 — 2°C 에 안감 없는 라이더가 기준이면 모든 조합이
    // 날씨 경계다. 그래서 몇 벌을 시도해, 날씨 판정이 통과인 코디를 경계인 코디보다 먼저
    // 고른다. 같은 판정 안에서는 최근에 덜 나온 기준 옷이 먼저다.
    // (날씨 통과, 기준 옷, build_final_outfit 결과)
    type Built = (String, Vec<ChatItem>, Vec<String>);
    let mut best: Option<(bool, &Clothing, Built)> = None;
    for anchor in anchors.into_iter().take(6) {
        let Some((desc, items, chosen_ids)) = build_final_outfit(anchor, clothes, ctx) else {
            continue;
        };
        let pieces: Vec<&Clothing> = chosen_ids
            .iter()
            .filter_map(|id| clothes.iter().find(|c| &c.id == id))
            .collect();
        let pass =
            today_fit_of(&pieces, temp) == crate::services::style_engine_v2::TodayFitLevel::Pass;
        if best.as_ref().is_none_or(|b| pass && !b.0) {
            best = Some((pass, anchor, (desc, items, chosen_ids)));
        }
        if pass {
            break;
        }
    }
    if let Some((_, anchor, (desc, mut items, chosen_ids))) = best {
        apply_wear(&mut items, clothes, ctx.temperature);
        let items_json: Vec<serde_json::Value> = items
            .iter()
            .map(|i| json!({"slot": i.slot, "name": i.name, "category": i.category, "owned": i.owned, "wear": i.wear}))
            .collect();
        let response = json!({ "anchor": anchor.name, "outfit": desc, "items": items_json });
        return (response.to_string(), items, chosen_ids);
    }
    (
        json!({"error": "no suitable outfit found"}).to_string(),
        Vec::new(),
        Vec::new(),
    )
}

fn build_final_outfit<'a>(
    anchor: &'a Clothing,
    clothes: &'a [Clothing],
    ctx: &OutfitContext<'_>,
) -> Option<(String, Vec<ChatItem>, Vec<String>)> {
    let anchor_cat = &anchor.category;
    let temp = ctx.temperature.unwrap_or(20.0);

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

    // 글램·엣지면 옷 슬롯마다 무드 요소가 있는 옷을 두 벌까지 후보에 더한다. 궁합 점수
    // 상위 몇 벌만 보면 무드 옷이 한 벌도 들지 못해, 아래 무드 게이트가 고를 것이 없었다.
    let with_mood = |mut v: Vec<&'a Clothing>, cat: &str| -> Vec<&'a Clothing> {
        if let Some(mood) = ctx.mood.filter(|m| crate::services::mood_gate::judges(*m)) {
            let extra: Vec<&Clothing> = clothes
                .iter()
                .filter(|c| c.category == cat && c.id != anchor.id)
                .filter(|c| is_weather_appropriate(c, temp))
                .filter(|c| crate::services::mood_gate::is_mood_piece(mood, c) == Some(true))
                .filter(|c| !v.iter().any(|x| x.id == c.id))
                .take(2)
                .collect();
            v.extend(extra);
        }
        v
    };
    let tops = if anchor_cat == "상의" {
        vec![anchor]
    } else {
        with_mood(slot_candidates("상의", 5), "상의")
    };
    let bottoms = if anchor_cat == "하의" {
        vec![anchor]
    } else {
        with_mood(slot_candidates("하의", 5), "하의")
    };
    let outers_pool = if anchor_cat == "아우터" {
        vec![anchor]
    } else {
        with_mood(slot_candidates("아우터", 4), "아우터")
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
                    let score = outfit_scorer::total_outfit_score_full(
                        anchor,
                        &outfit,
                        ctx.user,
                        ctx.feedback,
                        ctx.recent,
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
                        let score = outfit_scorer::total_outfit_score_full(
                            anchor,
                            &outfit,
                            ctx.user,
                            ctx.feedback,
                            ctx.recent,
                        );
                        combos.push((outfit, score));
                    }
                }
            }
        }
    }

    // 홈과 같은 오늘 입기 판정(`serving_ranker::compute_today_fit`). 기온을 알면 실패한
    // 조합은 빼고, 경계인 조합은 홈과 같은 만큼 깎는다. 남는 조합이 없으면 코디를 내지
    // 않는다 — 날씨에 맞지 않는 코디를 대신 내보내지 않는다. 상황(occasion)은 없으므로
    // 상황별 규칙(운동복 등)은 여기서 보지 않고, 보온·하체 노출만 본다.
    if let Some(t) = ctx.temperature {
        use crate::services::style_engine_v2::TodayFitLevel;
        combos = combos
            .into_iter()
            .filter_map(|(o, score)| match today_fit_of(&o, t) {
                TodayFitLevel::Fail => None,
                TodayFitLevel::Borderline => Some((
                    o,
                    score - crate::services::serving_ranker::WEATHER_BORDERLINE_PENALTY,
                )),
                _ => Some((o, score)),
            })
            .collect();
    }
    combos.sort_by_key(|a| std::cmp::Reverse(a.1));
    // 홈과 같은 무드 게이트(`services::mood_gate`). 글램이면 빛을 받는 소재나 조형적인
    // 실루엣이, 엣지면 형태나 가죽 옷이 하나라도 있는 조합 중 최고점을 고른다. 그런 조합이
    // 없으면 원래 최고점이다 — 빈 답보다 낫다.
    let reads = |o: &Vec<&Clothing>| {
        ctx.mood
            .and_then(|m| crate::services::mood_gate::reads_as_at(m, o, ctx.temperature))
            .unwrap_or(true)
    };
    let (best_outfit, best_score) = combos
        .iter()
        .find(|(o, _)| reads(o))
        .or_else(|| combos.first())?;

    tracing::info!(
        "final outfit (score={}): {}",
        best_score,
        best_outfit
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    );

    // 고른 아이템의 id — 호출자가 이력에 남겨 다음 추천에서 감점할 수 있게 넘긴다.
    let chosen_ids: Vec<String> = best_outfit.iter().map(|c| c.id.clone()).collect();

    let mut desc_parts = Vec::new();
    let mut items = Vec::new();
    for c in best_outfit.iter() {
        let Some(item) = ChatItem::from_clothing(c, true) else {
            continue;
        };
        desc_parts.push(format!("{}: {}", item.slot, c.name));
        items.push(item);
    }
    if !items.iter().any(|i| i.name == anchor.name) {
        let slot = slot_for_category(&anchor.category).unwrap_or("?");
        desc_parts.push(format!("{}: {}", slot, anchor.name));
        items.push(ChatItem {
            slot: slot.to_string(),
            category: anchor.category.clone(),
            name: anchor.name.clone(),
            owned: clothes.iter().any(|c| c.name == anchor.name),
            material: anchor.image_material(),
            wear: None,
        });
    }
    Some((desc_parts.join("\n"), items, chosen_ids))
}

/// 상담 코디 한 벌의 오늘 입기 판정. 홈의 후보와 같은 함수를 쓴다.
fn today_fit_of(
    outfit: &[&Clothing],
    temperature: f64,
) -> crate::services::style_engine_v2::TodayFitLevel {
    use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
    let slots = outfit
        .iter()
        .filter_map(|c| {
            let slot = match c.category.as_str() {
                "상의" => SlotKind::Top,
                "하의" => SlotKind::Bottom,
                "아우터" => SlotKind::Outer,
                "신발" => SlotKind::Shoes,
                "가방" => SlotKind::Bag,
                _ => return None,
            };
            Some(OutfitSlot {
                slot,
                clothing: (*c).clone(),
                seasons: Vec::new(),
                texture_worlds: Vec::new(),
            })
        })
        .collect();
    crate::services::serving_ranker::compute_today_fit(
        &OutfitContext {
            slots,
            situation: None,
        },
        temperature,
    )
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
    if crate::services::shortlist::shoe_weather(item, temp)
        == crate::services::shortlist::ShoeWeather::Unsuitable
    {
        return false;
    }
    let weight = item.weight.unwrap_or(Weight::Mid);
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;
    if temp >= 20.0 {
        // 벨벳은 울처럼 보온·무게가 있는 원단이다. 글램 시드에서 처음 들어왔고, 빠져 있으면
        // 27°C 에 벨벳 탑이 후보에 오른다.
        if mat == "wool" || mat == "flannel" || mat == "velvet" {
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
        // 가죽 상의·하의는 바람이 통하지 않는다. 엣지 시드의 레더 미디 스커트가 26°C 후보에
        // 올랐다(2026-10-17). 가죽 신발·가방은 그대로 둔다. 소재가 저장되지 않는 업로드 옷도
        // 이름으로 가린다(`mood_gate::is_leather_garment`).
        if item.category != "아우터" && crate::services::mood_gate::is_leather_garment(item) {
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

    fn chat_item(id: &str, name: &str, category: &str, material: &str, keywords: &str) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            color: Some("블랙".into()),
            thickness: crate::models::style_vocab::Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: None,
            style: None,
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: None,
            gender: None,
            style_mood: None,
            visual_weight: None,
            texture_depth: None,
            visual_weight_v2: None,
            texture_depth_v2: None,
            grounding_score: None,
            shadow_tone: None,
            silhouette_volume: None,
            material_primary: Some(material.into()),
            // 같은 세부 종류는 두 벌까지만 후보에 오르므로 아이템마다 다르게 둔다.
            sub_category: Some(id.into()),
            floating_score: None,
            strong_style_score: None,
            texture_keywords: Some(keywords.into()),
            created_at: now,
            updated_at: now,
        }
    }

    /// 상담의 코디도 홈과 같은 무드 게이트를 지난다. 글램이면 기본 아이템만 있는 조합
    /// 대신 글램 요소가 있는 조합을 고르고, 다른 무드는 원래 최고점 그대로다.
    #[test]
    fn chat_outfit_prefers_a_glam_piece_for_glam() {
        let clothes = vec![
            chat_item("t", "화이트 티", "상의", "cotton", "cotton jersey"),
            chat_item("b1", "블랙 슬랙스", "하의", "polyester", "wide leg"),
            chat_item("b2", "블랙 데님", "하의", "denim", "washed denim"),
            chat_item("b3", "블랙 새틴 스커트", "하의", "satin", "satin, midi"),
            chat_item("s", "블랙 앵클부츠", "신발", "leather", "ankle boot"),
            chat_item("g", "블랙 숄더백", "가방", "leather", "shoulder bag"),
        ];
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let recent = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let pick = |mood| {
            let ctx = OutfitContext {
                user: None,
                temperature: Some(18.0),
                feedback: &feedback,
                recent: &recent,
                mood,
            };
            build_final_outfit(&clothes[0], &clothes, &ctx)
                .unwrap()
                .1
                .into_iter()
                .map(|i| i.name)
                .collect::<Vec<_>>()
        };
        assert!(pick(Some(StyleGenre::Glam)).contains(&"블랙 새틴 스커트".to_string()));
        assert_eq!(pick(Some(StyleGenre::Minimal)), pick(None));
        // 게이트가 없으면 이 옷장의 최고점은 새틴 스커트가 아니다 — 위 판정이 게이트 덕이다.
        assert!(!pick(None).contains(&"블랙 새틴 스커트".to_string()));
    }

    /// 상담도 홈과 같은 오늘 입기 판정을 지난다. 1°C 에 미니·앵클부츠는 하체가 드러나 빠지고
    /// 같은 상체·코트의 슬랙스를 고른다. 코트에는 여며 입는 착용 상태가 붙는다. 날씨에 맞는
    /// 조합이 없으면 코디를 내지 않는다.
    #[test]
    fn chat_outfit_passes_the_same_weather_check() {
        use crate::models::style_vocab::Thickness;
        let mut coat = chat_item(
            "o",
            "블랙 아워글라스 롱 울 코트",
            "아우터",
            "wool",
            "hourglass silhouette, nipped waist, mid-calf length, insulated lining",
        );
        coat.thickness = Thickness::Thick;
        coat.sub_category = Some("롱코트".into());
        let mut top = chat_item(
            "t",
            "블랙 원숄더 저지 탑",
            "상의",
            "jersey",
            "one-shoulder neckline",
        );
        top.thickness = Thickness::Thin;
        let mini = chat_item(
            "m",
            "블랙 랩 미니스커트",
            "하의",
            "satin",
            "satin, mini length",
        );
        let slacks = chat_item("b", "블랙 와이드 슬랙스", "하의", "wool", "wide leg");
        let boots = chat_item("s", "블랙 앵클부츠", "신발", "leather", "ankle boot");
        let bag = chat_item("g", "블랙 새틴 클러치", "가방", "satin", "satin clutch");
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let recent = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let ctx = OutfitContext {
            user: None,
            temperature: Some(1.0),
            feedback: &feedback,
            recent: &recent,
            mood: Some(StyleGenre::Glam),
        };
        let clothes = vec![
            coat.clone(),
            top.clone(),
            mini.clone(),
            slacks,
            boots.clone(),
            bag.clone(),
        ];
        let (_, mut items, _) = build_final_outfit(&clothes[0], &clothes, &ctx).unwrap();
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"블랙 와이드 슬랙스"), "{names:?}");
        assert!(!names.contains(&"블랙 랩 미니스커트"));
        apply_wear(&mut items, &clothes, ctx.temperature);
        let outer = items.iter().find(|i| i.slot == "outer").unwrap();
        assert!(
            outer
                .wear
                .as_deref()
                .is_some_and(|w| w.contains("arms in the sleeves"))
        );
        assert!(items.iter().filter(|i| i.wear.is_some()).count() == 1);

        let only_mini = vec![coat, top, mini, boots, bag];
        assert!(build_final_outfit(&only_mini[0], &only_mini, &ctx).is_none());
    }

    /// 상담도 엣지 무드 게이트를 지난다. 기본 티·데님에 가죽 부츠·가방만 든 조합 대신 옷에
    /// 형태나 가죽이 있는 조합을 고른다. 2°C 에는 안감 없는 라이더 대신 보온 안감 코트를
    /// 고르고, 그 코트에 여며 입는 착용 상태가 붙는다.
    #[test]
    fn chat_outfit_reads_edgy_and_keeps_the_weather_check() {
        use crate::models::style_vocab::Thickness;
        let mut coat = chat_item(
            "o1",
            "차콜 비대칭 여밈 롱 울 코트",
            "아우터",
            "wool",
            "heavy wool, asymmetric overlapping front, mid-calf length, insulated lining",
        );
        coat.thickness = Thickness::Thick;
        coat.sub_category = Some("롱코트".into());
        let mut rider = chat_item(
            "o2",
            "블랙 레더 라이더 자켓",
            "아우터",
            "leather",
            "cropped biker",
        );
        rider.sub_category = Some("라이더자켓".into());
        let clothes = vec![
            chat_item(
                "t1",
                "화이트 코튼 티셔츠",
                "상의",
                "cotton",
                "cotton jersey",
            ),
            chat_item(
                "t2",
                "블랙 리브드 슬림 니트",
                "상의",
                "knit",
                "ribbed knit, slim fit",
            ),
            chat_item(
                "b1",
                "인디고 와이드 데님",
                "하의",
                "denim",
                "denim, wide leg",
            ),
            chat_item(
                "b2",
                "차콜 울 배럴 레그 팬츠",
                "하의",
                "wool",
                "curved barrel leg",
            ),
            chat_item(
                "s",
                "블랙 레더 컴뱃 부츠",
                "신발",
                "leather",
                "lace-up, lug sole",
            ),
            chat_item("g", "블랙 레더 숄더백", "가방", "leather", "angular shape"),
            coat,
            rider,
        ];
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let recent = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let pick = |t: f64| {
            let ctx = OutfitContext {
                user: None,
                temperature: Some(t),
                feedback: &feedback,
                recent: &recent,
                mood: Some(StyleGenre::Edgy),
            };
            let (_, mut items, _) = build_final_outfit(&clothes[4], &clothes, &ctx).unwrap();
            apply_wear(&mut items, &clothes, ctx.temperature);
            items
        };
        let warm = pick(18.0);
        let warm_clothes: Vec<&Clothing> = warm
            .iter()
            .filter_map(|i| clothes.iter().find(|c| c.name == i.name))
            .collect();
        assert_eq!(
            crate::services::mood_gate::reads_as(StyleGenre::Edgy, &warm_clothes),
            Some(true),
            "{:?}",
            warm.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
        let cold = pick(2.0);
        let names: Vec<&str> = cold.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"차콜 비대칭 여밈 롱 울 코트"), "{names:?}");
        assert!(!names.contains(&"블랙 레더 라이더 자켓"));
        let outer = cold.iter().find(|i| i.slot == "outer").unwrap();
        assert!(
            outer
                .wear
                .as_deref()
                .is_some_and(|w| w.contains("arms in the sleeves"))
        );
    }

    fn profile_stub() -> crate::models::user_profile::UserStyleProfile {
        crate::models::user_profile::UserStyleProfile {
            user_id: "u".into(),
            height_cm: None,
            weight_kg: None,
            upper_body: None,
            calves: None,
            preferred_fit: None,
            leg_length: None,
            preferred_top_size: None,
            needs_grounded_shoes: false,
            prefers_weighted_bag: false,
            low_profile_only_occasional: false,
            medium_volume_runner_bonus: false,
            denim_bridge_bonus: false,
            likes_texture_depth: false,
            likes_melange: false,
            likes_suede: false,
            likes_washed_denim: false,
            likes_mocha_brown: false,
            likes_heather_gray: false,
            dislikes_flat_beige: false,
            dislikes_military_cosplay: false,
            dislikes_bright_colors: false,
            commute: None,
            walking_amount: None,
            comfort_priority: None,
        }
    }

    /// 상담 모델이 받는 도구에 기준 아이템 없는 추천이 있다. 기준 아이템 추천(`get_outfit`)은
    /// 예전처럼 기준 이름을 요구한다 — 빈 이름이면 옷장 첫 옷에 부분 일치하기 때문이다.
    #[test]
    fn chat_tools_offer_an_anchorless_outfit() {
        let tools = chat_tools();
        let schema = |name: &str| {
            let t = tools.iter().find(|t| t.name == name).unwrap();
            t.input_schema.clone()
        };
        let today = schema("get_outfit_for_today");
        assert_eq!(today["required"], json!([]));
        assert_eq!(
            schema("get_outfit")["required"],
            json!(["user_query", "anchor_name"])
        );
        assert!(tools.iter().all(|t| t.name != "swap_item"));
    }

    /// 기준 아이템 없는 상담 추천(`get_outfit_for_today` 가 부르는 함수): 엣지면 무드 요소가
    /// 있는 옷을 기준으로 삼고, 날씨 판정·무드 게이트·착용 상태가 기준 아이템 추천과 같다.
    /// 2°C 에는 안감 없는 라이더 대신 안감 코트를 여며 입고, 26°C 에는 아우터·가죽 하의가
    /// 없다. 같은 옷만 기준이 되지 않게 최근에 덜 나온 옷부터 쓴다.
    #[test]
    fn today_outfit_follows_mood_weather_and_recency() {
        use crate::models::style_vocab::Thickness;
        let mut coat = chat_item(
            "o1",
            "차콜 비대칭 여밈 롱 울 코트",
            "아우터",
            "wool",
            "heavy wool, asymmetric overlapping front, mid-calf length, insulated lining",
        );
        coat.thickness = Thickness::Thick;
        coat.sub_category = Some("롱코트".into());
        let mut rider = chat_item(
            "o2",
            "블랙 레더 라이더 자켓",
            "아우터",
            "leather",
            "cropped biker",
        );
        rider.sub_category = Some("라이더자켓".into());
        let mut asym = chat_item(
            "t1",
            "차콜 비대칭 헴 저지 롱슬리브",
            "상의",
            "jersey",
            "asymmetric hem, long sleeves",
        );
        asym.thickness = Thickness::Thin;
        let mut skirt = chat_item(
            "b3",
            "블랙 레더 미디 스커트",
            "하의",
            "leather",
            "matte leather, midi length",
        );
        skirt.sub_category = Some("스커트".into());
        let mut sandals = chat_item(
            "s2",
            "블랙 레더 피셔맨 샌들",
            "신발",
            "leather",
            "woven straps",
        );
        sandals.sub_category = Some("샌들".into());
        let clothes = vec![
            chat_item(
                "t2",
                "화이트 코튼 티셔츠",
                "상의",
                "cotton",
                "cotton jersey",
            ),
            chat_item(
                "t3",
                "블랙 리브드 슬림 니트",
                "상의",
                "knit",
                "ribbed knit, slim fit",
            ),
            asym,
            chat_item(
                "b1",
                "인디고 와이드 데님",
                "하의",
                "denim",
                "denim, wide leg",
            ),
            chat_item(
                "b2",
                "차콜 울 배럴 레그 팬츠",
                "하의",
                "wool",
                "curved barrel leg",
            ),
            skirt,
            chat_item(
                "s1",
                "블랙 레더 컴뱃 부츠",
                "신발",
                "leather",
                "lace-up, lug sole",
            ),
            sandals,
            chat_item("g", "블랙 레더 숄더백", "가방", "leather", "angular shape"),
            coat,
            rider,
        ];
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let none = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let run = |t: f64, mood, recent: &outfit_scorer::RecentHistory| {
            let ctx = OutfitContext {
                user: None,
                temperature: Some(t),
                feedback: &feedback,
                recent,
                mood,
            };
            let (out, items, ids) = today_outfit(&clothes, &ctx);
            let anchor = serde_json::from_str::<serde_json::Value>(&out).unwrap()["anchor"]
                .as_str()
                .unwrap_or("")
                .to_string();
            (anchor, items, ids)
        };
        let pieces = |items: &[ChatItem]| -> Vec<&Clothing> {
            items
                .iter()
                .filter_map(|i| clothes.iter().find(|c| c.name == i.name))
                .collect()
        };

        let (anchor, cold, ids) = run(2.0, Some(StyleGenre::Edgy), &none);
        assert!(!ids.is_empty());
        let a = clothes.iter().find(|c| c.name == anchor).unwrap();
        assert!(crate::services::mood_gate::is_edgy_piece(a), "{anchor}");
        assert_eq!(
            crate::services::mood_gate::reads_as(StyleGenre::Edgy, &pieces(&cold)),
            Some(true)
        );
        let names: Vec<&str> = cold.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"차콜 비대칭 여밈 롱 울 코트"), "{names:?}");
        assert!(!names.contains(&"블랙 레더 라이더 자켓"));
        assert!(
            cold.iter()
                .find(|i| i.slot == "outer")
                .and_then(|i| i.wear.as_deref())
                .is_some_and(|w| w.contains("arms in the sleeves"))
        );

        let (_, hot, _) = run(26.0, Some(StyleGenre::Edgy), &none);
        let names: Vec<&str> = hot.iter().map(|i| i.name.as_str()).collect();
        assert!(hot.iter().all(|i| i.slot != "outer"), "{names:?}");
        assert!(!names.contains(&"블랙 레더 미디 스커트"));
        assert!(!names.contains(&"차콜 울 배럴 레그 팬츠"));
        assert_eq!(
            crate::services::mood_gate::reads_as(StyleGenre::Edgy, &pieces(&hot)),
            Some(true)
        );

        // 최근에 가장 많이 나온 기준 옷은 다음에 뒤로 간다.
        let mut seen = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let first = run(16.0, Some(StyleGenre::Edgy), &none).0;
        let first_id = clothes.iter().find(|c| c.name == first).unwrap().id.clone();
        seen.item_freq.insert(first_id, 3);
        assert_ne!(run(16.0, Some(StyleGenre::Edgy), &seen).0, first);

        // 엣지에서 최근 이력이 같으면 보이는 색이 있는 기준 옷이 먼저다(시드 밖의 색 이름도
        // 같다). 이름순으로는 뒤인 코럴 니트가 앞선다. 밝은색을 싫어한다고 적은 프로필이면
        // 색을 앞세우지 않는다.
        let mut coloured = clothes.clone();
        let mut coral = chat_item(
            "t9",
            "코럴 비대칭 넥 니트",
            "상의",
            "knit",
            "asymmetric neckline",
        );
        coral.color = Some("코럴".into());
        coral.saturation = Some(crate::models::style_vocab::Saturation::High);
        coloured.push(coral);
        let anchor_at_16 = |user: Option<&crate::models::user_profile::UserStyleProfile>| {
            let ctx16 = OutfitContext {
                user,
                temperature: Some(16.0),
                feedback: &feedback,
                recent: &none,
                mood: Some(StyleGenre::Edgy),
            };
            serde_json::from_str::<serde_json::Value>(&today_outfit(&coloured, &ctx16).0).unwrap()
                ["anchor"]
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(anchor_at_16(None), "코럴 비대칭 넥 니트");
        let dislikes = crate::models::user_profile::UserStyleProfile {
            dislikes_bright_colors: true,
            ..profile_stub()
        };
        assert_ne!(anchor_at_16(Some(&dislikes)), "코럴 비대칭 넥 니트");

        // 따뜻한 날에는 두꺼운 컬러 코트가 아니라 컬러 상의를 먼저 시도한다. 이름순으로는
        // 코트가 앞이다.
        let mut warm_set = coloured.clone();
        let mut cobalt_coat = chat_item(
            "o9",
            "가 코발트 보온 울 코트",
            "아우터",
            "wool",
            "asymmetric wrap front, insulated lining",
        );
        cobalt_coat.color = Some("코발트".into());
        cobalt_coat.saturation = Some(crate::models::style_vocab::Saturation::High);
        cobalt_coat.thickness = Thickness::Thick;
        warm_set.push(cobalt_coat);
        let ctx19 = OutfitContext {
            user: None,
            temperature: Some(19.0),
            feedback: &feedback,
            recent: &none,
            mood: Some(StyleGenre::Edgy),
        };
        let anchor19 =
            serde_json::from_str::<serde_json::Value>(&today_outfit(&warm_set, &ctx19).0).unwrap()
                ["anchor"]
                .as_str()
                .unwrap_or("")
                .to_string();
        assert_eq!(anchor19, "코럴 비대칭 넥 니트");

        // 무드 요소를 판단하지 않는 무드도 코디를 만든다.
        assert!(!run(16.0, Some(StyleGenre::Minimal), &none).2.is_empty());
    }

    /// 상담의 팝 코디도 홈과 같은 게이트를 지난다 — 기준 옷이 선명한 옷이어도 그 옷 한 벌에
    /// 기본색만 맞춘 조합은 내지 않고, 다른 색 계열 옷이나 배색 옷이 함께인 조합을 고른다.
    /// 8°C·2°C 에는 여민 코트 속 상의를 근거로 세지 않는다(코트나 하의의 색이 보여야 한다).
    #[test]
    fn today_outfit_reads_pop_with_visible_colour() {
        use crate::models::style_vocab::{Saturation, Thickness};
        let coloured = |id: &str, name: &str, cat: &str, color: &str, kw: &str| {
            let mut c = chat_item(id, name, cat, "cotton", kw);
            c.color = Some(color.into());
            c.saturation = Some(Saturation::High);
            c
        };
        let plain = |id: &str, name: &str, cat: &str, color: &str| {
            let mut c = chat_item(id, name, cat, "cotton", "plain");
            c.color = Some(color.into());
            c.saturation = Some(Saturation::Low);
            c
        };
        let mut navy_coat = plain("o1", "네이비 보온 울 코트", "아우터", "네이비");
        navy_coat.texture_keywords = Some("heavy wool, knee-length, insulated lining".into());
        navy_coat.thickness = Thickness::Thick;
        let mut check_coat = coloured(
            "o2",
            "그린 레드 체크 울 코트",
            "아우터",
            "그린/레드",
            "heavy wool, bold check in green and red, knee-length, insulated lining",
        );
        check_coat.thickness = Thickness::Thick;
        let clothes = vec![
            coloured("t1", "코발트 크롭 니트", "상의", "코발트", "soft knit"),
            coloured(
                "t2",
                "레드 핑크 컬러블록 스웨트셔츠",
                "상의",
                "레드/핑크",
                "colour-block panels in red and pink",
            ),
            plain("t3", "화이트 코튼 티셔츠", "상의", "화이트"),
            plain("t4", "블랙 슬림 니트", "상의", "블랙"),
            coloured(
                "b1",
                "체리레드 미디 스커트",
                "하의",
                "체리레드",
                "a-line, midi",
            ),
            coloured(
                "b2",
                "켈리그린 코듀로이 팬츠",
                "하의",
                "켈리그린",
                "full length",
            ),
            plain("b3", "인디고 와이드 데님", "하의", "인디고"),
            plain("s1", "화이트 스니커즈", "신발", "화이트"),
            plain("s2", "블랙 앵클부츠", "신발", "블랙"),
            plain("g", "블랙 숄더백", "가방", "블랙"),
            navy_coat,
            check_coat,
        ];
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let none = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        for t in [19.6, 8.0, 2.0] {
            let ctx = OutfitContext {
                user: None,
                temperature: Some(t),
                feedback: &feedback,
                recent: &none,
                mood: Some(StyleGenre::Pop),
            };
            let (_, items, ids) = today_outfit(&clothes, &ctx);
            assert!(!ids.is_empty(), "{t}");
            let pieces: Vec<&Clothing> = ids
                .iter()
                .filter_map(|id| clothes.iter().find(|c| &c.id == id))
                .collect();
            let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
            assert_eq!(
                crate::services::mood_gate::reads_as_at(StyleGenre::Pop, &pieces, Some(t)),
                Some(true),
                "{t}: {names:?}"
            );
            // 같은 착장을 기온 없이 보면(상의도 보인다고 보면) 당연히 팝이다 — 기온을 넣은
            // 판정이 더 엄격하다.
            assert_eq!(
                crate::services::mood_gate::reads_as(StyleGenre::Pop, &pieces),
                Some(true)
            );
        }
    }

    /// 팝에서 말없이 "하의만 바꿔줘" 하면 팝이 깨지지 않는 하의를 고른다. 그런 하의가 없으면
    /// 궁합 점수 그대로이고, 다른 무드는 예전과 같은 옷을 고른다.
    #[test]
    fn a_silent_pop_swap_keeps_the_outfit_pop() {
        use crate::models::style_vocab::Saturation;
        use crate::models::style_vocab::{Role, Tone};
        // 시드와 같은 역할·톤: 기본색은 베이스, 색 옷은 약한포인트.
        let colour = |id: &str, name: &str, cat: &str, color: &str| {
            let mut c = chat_item(id, name, cat, "cotton", "plain");
            c.color = Some(color.into());
            c.saturation = Some(Saturation::High);
            c.role = Some(Role::SoftAccent);
            c.tone = Some(Tone::Mid);
            c
        };
        let plain = |id: &str, name: &str, cat: &str, color: &str| {
            let mut c = chat_item(id, name, cat, "cotton", "plain");
            c.color = Some(color.into());
            c.saturation = Some(Saturation::Low);
            c.role = Some(Role::Base);
            c.tone = Some(if color == "블랙" {
                Tone::Dark
            } else {
                Tone::Bright
            });
            c
        };
        let tee = plain("t", "화이트 코튼 티셔츠", "상의", "화이트");
        let jacket = colour("o", "레드 크롭 트러커 재킷", "아우터", "레드");
        let kept = vec![&tee, &jacket];
        let neutral_only = vec![
            plain("b1", "화이트 와이드 데님 팬츠", "하의", "화이트"),
            plain("b2", "블랙 와이드 슬랙스", "하의", "블랙"),
        ];
        let mut with_colour = neutral_only.clone();
        with_colour.push(colour("b3", "코발트 와이드 팬츠", "하의", "코발트"));
        let pick = |pool: &[Clothing], mood| {
            best_fit_swap(pool, &kept, mood, Some(19.6)).map(|c| c.name.clone())
        };
        assert_eq!(
            pick(&with_colour, Some(StyleGenre::Pop)).as_deref(),
            Some("코발트 와이드 팬츠")
        );
        // 재현 조건: 무드 없이 궁합 점수만 보면 기본색 하의를 고른다.
        assert_ne!(
            pick(&with_colour, None).as_deref(),
            Some("코발트 와이드 팬츠")
        );
        // 팝을 지킬 하의가 없으면 궁합 점수로 고른다(빈 답이 아니다).
        assert_eq!(
            pick(&neutral_only, Some(StyleGenre::Pop)),
            pick(&neutral_only, None)
        );
        // 다른 무드는 무드 없는 선택과 같다.
        for m in [None, Some(StyleGenre::Edgy), Some(StyleGenre::Street)] {
            assert_eq!(pick(&with_colour, m), pick(&with_colour, None));
        }
    }

    const GLAM_ITEMS: &str = "top/inner: 에메랄드 새틴 드레이프 블라우스 (satin, fluid satin, draped cowl neckline), \
         pants: 블랙 와이드 슬랙스 (polyester, wide leg)";

    /// 글램 프롬프트의 나이·메이크업·표정·회피 조건이 서로 부딪치지 않는다. 공통 문장을 둔
    /// 채 반대 지시를 덧붙이지 않고, 그 자리를 무드가 고른 문장으로 바꿨는지 본다.
    #[test]
    fn glam_prompt_has_no_conflicting_face_or_avoid_terms() {
        for h in [0u64, 1, 42, 1 << 56, u64::MAX, 0x1234_5678_9abc_def0] {
            let p = build_image_prompt(StyleGenre::Glam, GLAM_ITEMS, h);
            for gone in [
                "in her twenties",
                "minimal everyday makeup",
                "not posed for anyone",
                "not aware of the camera",
                "excessive makeup",
                "fashion-model posing",
                "excessive gloss",
                "muted neutral palette",
                "slightly desaturated",
                "restrained saturation",
                "understated",
            ] {
                assert!(!p.contains(gone), "{h}: {gone}");
            }
            assert!(p.contains("every garment keeps its own colour and saturation"));
            assert!(p.contains("between her mid-twenties and mid-thirties"));
            assert!(p.contains("aware of the camera and at ease with it"));
            // 메이크업 포인트는 한 곳뿐이다.
            let eyes = p.contains("refined eye look");
            let lips = p.contains("soft lip colour");
            assert!(eyes ^ lips, "{h}: 눈 {eyes} 입술 {lips}");
            // 과한 화장·연출을 막는 조건은 구체적으로 남아 있다.
            assert!(p.contains("strong eye makeup and strong lip colour together"));
            assert!(p.contains("pouting lips"));
            assert!(p.contains("blown-out highlights on satin or metallic fabric"));
            // 소품과 노출을 더하지 않는다.
            assert!(p.contains("necklaces"));
            assert!(p.contains("a hemline shorter or a neckline lower than listed"));
        }
    }

    /// 메이크업 선택은 해시로 정해져 같은 입력에서 늘 같고, 두 가지가 모두 나온다.
    #[test]
    fn glam_makeup_choice_is_stable_and_both_occur() {
        assert_eq!(glam_makeup(7), glam_makeup(7));
        assert_ne!(glam_makeup(0), glam_makeup(1 << 56));
        let p1 = build_image_prompt(StyleGenre::Glam, GLAM_ITEMS, 99);
        let p2 = build_image_prompt(StyleGenre::Glam, GLAM_ITEMS, 99);
        assert_eq!(p1, p2);
    }

    /// 광택 문장은 빛을 받는 소재가 목록에 있을 때만 들어간다. 무광 코디를 글램이라고
    /// 반짝이게 그리면 안 된다.
    #[test]
    fn glam_sheen_line_follows_the_listed_fabrics() {
        let shiny = build_image_prompt(StyleGenre::Glam, GLAM_ITEMS, 5);
        assert!(shiny.contains("soft, natural reflections that follow the drape"));
        let matte = build_image_prompt(
            StyleGenre::Glam,
            "top/inner: 블랙 원숄더 저지 탑 (jersey, matte stretch jersey, one-shoulder neckline)",
            5,
        );
        assert!(!matte.contains("natural reflections"));
        assert!(matte.contains("matte fabrics stay matte, with no sheen added"));
    }

    /// 글램을 밤·클럽·네온으로 굳히지 않는다. 장소는 시간을 말하지 않고, 빛은 저녁과 낮이
    /// 모두 나온다.
    #[test]
    fn glam_scenes_are_not_fixed_to_night() {
        let (places, own_light) = genre_setting(StyleGenre::Glam);
        for p in places {
            for w in ["night", "club", "neon", "dusk", "evening"] {
                assert!(!p.contains(w), "{p}");
            }
        }
        let lights: std::collections::HashSet<&str> = (0..256u64)
            .map(|i| pick_light(StyleGenre::Glam, i << 40))
            .collect();
        assert!(lights.contains(own_light));
        assert!(lights.contains("bright open daylight"));
    }

    /// 글램은 같은 옷이라도 다른 무드와 캐시 키가 갈린다. 같은 입력이면 같은 키다.
    #[test]
    fn glam_has_its_own_cache_key() {
        let glam = image_cache_keys(GLAM_ITEMS, StyleGenre::Glam);
        let chic = image_cache_keys(GLAM_ITEMS, StyleGenre::ModernChic);
        assert_eq!(glam.0, chic.0, "착장 해시는 무드와 무관하다");
        assert_ne!(glam.1, chic.1, "프롬프트 해시는 갈린다");
        assert_eq!(glam, image_cache_keys(GLAM_ITEMS, StyleGenre::Glam));
    }

    const EDGY_ITEMS: &str = "top/inner: 코발트 비대칭 넥 리브 니트 (knit, ribbed wool-blend knit, asymmetric neckline cut lower on one side), \
         pants: 블랙 레더 미디 스커트 (leather, smooth matte leather, straight cut, midi length, back slit), \
         outerwear: 차콜 비대칭 여밈 롱 울 코트 (wool, heavy wool, structured shoulders, insulated lining; worn closed with its own fastening, arms in the sleeves), \
         shoes: 블랙 레더 컴뱃 부츠 (leather, lace-up front, chunky lug sole)";

    /// 엣지 프롬프트: 옷의 색을 누르는 팔레트 문장이 없고, 무광 가죽을 번들거리게 하지
    /// 않으며, 목록에 없는 피어싱·타투·체인·찢김을 막는다. 표정 문장이 서로 부딪치지 않고,
    /// 착용 상태는 목록 그대로 들어간다. 글램 문장은 섞이지 않는다.
    #[test]
    fn edgy_prompt_keeps_colour_surface_and_listed_pieces() {
        for h in [0u64, 1, 42, 1 << 40, 3 << 48, u64::MAX] {
            let p = build_image_prompt(StyleGenre::Edgy, EDGY_ITEMS, h);
            assert!(p.contains(EDGY_ITEMS));
            assert!(p.contains("worn closed with its own fastening, arms in the sleeves"));
            assert!(!p.contains("muted neutral palette"));
            assert!(!p.contains("restrained saturation"));
            assert!(p.contains("true-to-life saturation"));
            assert!(p.contains("matte leather stays matte"));
            for w in [
                "piercings",
                "tattoos",
                "studs",
                "rips",
                "graffiti",
                "neon lighting",
            ] {
                assert!(p.contains(w), "{w}");
            }
            // 공통 표정·구도 문장과 엣지 문장이 함께 있지 않다.
            assert!(!p.contains("not posed for anyone"));
            assert!(!p.contains("is not aware of the camera"));
            assert!(p.contains("confident without performing"));
            for glam_only in ["dressed to be noticed", "satin, velvet and", "champagne"] {
                assert!(!p.contains(glam_only), "{glam_only}");
            }
            // 걷다가 돌아보기를 포함한 공통 자세와 머리부터 신발까지 보이는 구도는 그대로다.
            assert!(p.contains("Full body visible from head to shoes"));
        }
    }

    /// 엣지의 장소가 어두운 골목·그래피티·네온으로 수렴하지 않는다. 밝은 장소와 색이 있는
    /// 장소가 섞이고, 같은 입력이면 같은 장소다(캐시 키가 안정적이다).
    #[test]
    fn edgy_scenes_are_varied_and_not_grungy() {
        let (places, own_light) = genre_setting(StyleGenre::Edgy);
        assert!(places.len() >= 4);
        for p in places {
            for w in ["alley", "graffiti", "neon", "night"] {
                assert!(!p.contains(w), "{p}");
            }
        }
        assert!(
            places
                .iter()
                .any(|p| p.contains("colour") || p.contains("coloured"))
        );
        assert!(!own_light.contains("night"));
        let picked: std::collections::HashSet<&str> = (0..64u64)
            .map(|h| pick_setting(StyleGenre::Edgy, h))
            .collect();
        assert!(picked.len() >= 6, "{picked:?}");
        assert_eq!(
            image_cache_keys(EDGY_ITEMS, StyleGenre::Edgy),
            image_cache_keys(EDGY_ITEMS, StyleGenre::Edgy)
        );
        assert_ne!(
            image_cache_keys(EDGY_ITEMS, StyleGenre::Edgy).1,
            image_cache_keys(EDGY_ITEMS, StyleGenre::ModernChic).1
        );
    }

    /// 홈과 같은 기준: 팝의 온화한 날(19.6°C)에는 상담 오늘 코디와 말없는 바꾸기도 겨울 패딩을
    /// 쓰지 않는다. 찬 날(8·2°C)에는 남는다. 기온을 모르거나 다른 무드면 예전 그대로다.
    #[test]
    fn mild_pop_days_keep_winter_outers_out_of_chat_picks() {
        use crate::models::style_vocab::{Role, Saturation, Thickness, Tone};
        let mut puffer = chat_item(
            "o1",
            "핑크 오렌지 컬러블록 패딩",
            "아우터",
            "nylon",
            "colour-block puffer in pink and orange, down fill, hip-length",
        );
        puffer.color = Some("핑크/오렌지".into());
        puffer.saturation = Some(Saturation::High);
        puffer.thickness = Thickness::Thick;
        puffer.role = Some(Role::Accent);
        let mut trucker = chat_item("o2", "레드 크롭 트러커 재킷", "아우터", "cotton", "cropped");
        trucker.color = Some("레드".into());
        trucker.saturation = Some(Saturation::High);
        trucker.role = Some(Role::SoftAccent);
        trucker.tone = Some(Tone::Mid);
        let plain = |id: &str, name: &str, cat: &str, color: &str| {
            let mut c = chat_item(id, name, cat, "cotton", "plain");
            c.color = Some(color.into());
            c.saturation = Some(Saturation::Low);
            c.role = Some(Role::Base);
            c
        };
        let mut knit = plain("t1", "블랙 슬림 니트", "상의", "블랙");
        knit.thickness = Thickness::Medium;
        let clothes = vec![
            knit,
            plain("t2", "화이트 코튼 티셔츠", "상의", "화이트"),
            plain("b1", "인디고 와이드 데님", "하의", "인디고"),
            plain("b2", "블랙 와이드 슬랙스", "하의", "블랙"),
            plain("s1", "블랙 앵클부츠", "신발", "블랙"),
            plain("g", "블랙 숄더백", "가방", "블랙"),
            puffer.clone(),
        ];
        let feedback = outfit_scorer::FeedbackContext {
            item_adj: Default::default(),
            preference: Default::default(),
        };
        let none = outfit_scorer::RecentHistory {
            item_freq: Default::default(),
        };
        let names_at = |t: Option<f64>, mood| {
            let ctx = OutfitContext {
                user: None,
                temperature: t,
                feedback: &feedback,
                recent: &none,
                mood,
            };
            today_outfit(&clothes, &ctx)
                .1
                .into_iter()
                .map(|i| i.name)
                .collect::<Vec<_>>()
        };
        let p = "핑크 오렌지 컬러블록 패딩".to_string();
        // 재현 조건: 패딩이 유일한 팝 옷이라, 무드 판정만 보면 19.6°C 에도 기준 옷이 된다.
        assert!(crate::services::mood_gate::is_pop_piece(&puffer));
        let mild = names_at(Some(19.6), Some(StyleGenre::Pop));
        assert!(!mild.is_empty() && !mild.contains(&p), "{mild:?}");
        for t in [8.0, 2.0] {
            let cold = names_at(Some(t), Some(StyleGenre::Pop));
            assert!(cold.contains(&p), "{t}: {cold:?}");
        }

        // 말없는 바꾸기(아우터): 19.6°C 팝이면 패딩 대신 트러커, 찬 날에는 패딩도 후보다.
        let tee = &clothes[1];
        let denim = &clothes[2];
        let kept = vec![tee, denim];
        let pool = vec![puffer.clone(), trucker.clone()];
        let pick = |mood, t| best_fit_swap(&pool, &kept, mood, t).map(|c| c.name.clone());
        assert_eq!(
            pick(Some(StyleGenre::Pop), Some(19.6)).as_deref(),
            Some("레드 크롭 트러커 재킷")
        );
        let only_puffer = vec![puffer.clone()];
        assert_eq!(
            best_fit_swap(&only_puffer, &kept, Some(StyleGenre::Pop), Some(2.0))
                .map(|c| c.name.as_str()),
            Some("핑크 오렌지 컬러블록 패딩")
        );
        assert!(best_fit_swap(&only_puffer, &kept, Some(StyleGenre::Pop), Some(19.6)).is_none());
        // 기온을 모르거나 다른 무드면 빼지 않는다.
        for (mood, t) in [
            (Some(StyleGenre::Pop), None),
            (Some(StyleGenre::Edgy), Some(19.6)),
            (None, Some(19.6)),
        ] {
            assert!(best_fit_swap(&only_puffer, &kept, mood, t).is_some());
        }
    }

    const POP_ITEMS: &str = "top/inner: 코발트 블루 크롭 니트 (soft cotton-blend knit, cropped length, crew neck, long sleeves), \
         pants: 블랙 와이드 슬랙스 (polyester, wide leg, clean drape), \
         outerwear: 그린 레드 체크 울 코트 (heavy wool, bold check in green and red, knee-length, insulated lining; worn closed with its own fastening, arms in the sleeves), \
         shoes: 블랙 레더 앵클부츠 (leather, block heel ankle boot)";

    /// 팝 프롬프트: 옷의 색을 누르는 팔레트 문장이 없고, 목록의 색·무늬를 그대로 두며 새
    /// 프린트·로고·소품을 막는다. 여민 코트는 속옷 색을 보이려고 열지 않는다(2026-10-17 실측
    /// 8°C 체크 코트 사진이 열린 채 나왔다). 표정은 밝지만 광고 미소·아이돌 포즈는 아니다.
    /// 공통 얼굴(실제 성인, 개인의 얼굴)과 머리부터 신발까지의 구도는 그대로다.
    #[test]
    fn pop_prompt_keeps_listed_colours_and_closed_coats() {
        for h in [0u64, 1, 42, 1 << 40, 3 << 48, u64::MAX] {
            let p = build_image_prompt(StyleGenre::Pop, POP_ITEMS, h);
            assert!(p.contains(POP_ITEMS));
            assert!(p.contains("worn closed with its own fastening, arms in the sleeves"));
            assert!(p.contains("never open it to show the colour of what is underneath"));
            assert!(p.contains("no new print, logo, slogan or decoration added"));
            assert!(!p.contains("muted neutral palette"));
            assert!(!p.contains("restrained saturation"));
            assert!(p.contains("true-to-life saturation"));
            assert!(p.contains("in her twenties or early thirties"));
            assert!(p.contains("never a posed advertising grin"));
            for w in [
                "amusement park",
                "candy shop",
                "graffiti",
                "neon lighting",
                "costume or cosplay",
                "childish",
                "idol-style finger hearts",
                "coloured eyeshadow matched to the clothes",
                "added prints or logos",
            ] {
                assert!(p.contains(w), "{w}");
            }
            assert!(!p.contains("not posed for anyone"));
            assert!(p.contains("distinctive individual facial features"));
            assert!(p.contains("Full body visible from head to shoes"));
            for other in ["dressed to be noticed", "matte leather stays matte"] {
                assert!(!p.contains(other), "{other}");
            }
        }
    }

    /// 팝의 장소는 색 건축·그래픽 타일 카페·디자인 서점·계단·갤러리로 고르게 나뉘고, 놀이공원·
    /// 사탕 가게·그래피티·네온으로 수렴하지 않는다. 같은 입력이면 같은 장소·캐시 키다.
    #[test]
    fn pop_scenes_are_varied_and_not_kitsch() {
        let (places, own_light) = genre_setting(StyleGenre::Pop);
        assert!(places.len() >= 4);
        for p in places {
            for w in ["amusement", "candy", "graffiti", "neon", "night"] {
                assert!(!p.contains(w), "{p}");
            }
        }
        assert!(own_light.contains("colours of the clothes true"));
        let picked: std::collections::HashSet<&str> = (0..64u64)
            .map(|h| pick_setting(StyleGenre::Pop, h))
            .collect();
        assert!(picked.len() >= 6, "{picked:?}");
        assert_eq!(
            image_cache_keys(POP_ITEMS, StyleGenre::Pop),
            image_cache_keys(POP_ITEMS, StyleGenre::Pop)
        );
        assert_ne!(
            image_cache_keys(POP_ITEMS, StyleGenre::Pop).1,
            image_cache_keys(POP_ITEMS, StyleGenre::Street).1
        );
    }

    /// 다른 무드의 얼굴·구도·회피 문장은 글램 작업 전과 같다. 이 문장이 바뀌면 그 무드의
    /// 저장된 사진이 캐시에서 전부 빗나간다(2026-10-16 12개 무드 × 8개 입력 대조로 확인).
    #[test]
    fn other_moods_keep_their_shared_face_and_avoid_text() {
        for g in StyleGenre::ALL
            .iter()
            .filter(|g| !matches!(g, StyleGenre::Glam | StyleGenre::Edgy | StyleGenre::Pop))
        {
            let p = build_image_prompt(*g, GLAM_ITEMS, 42);
            assert!(p.contains("woman in her twenties with distinctive"), "{g}");
            assert!(
                p.contains(
                    "Groomed natural brows, minimal everyday makeup, natural lips, realistic eyes"
                ),
                "{g}"
            );
            assert!(
                p.contains(
                    "Her expression is relaxed and spontaneous, not posed for anyone. Hairstyle:"
                ),
                "{g}"
            );
            assert!(
                p.contains("going about her own business and is not aware of the camera"),
                "{g}"
            );
            assert!(p.contains("oversharpening, excessive makeup, generic stock-photo smile, fashion-model posing,"), "{g}");
            assert!(
                p.contains("fake metallic reflections, excessive gloss,"),
                "{g}"
            );
            assert!(
                p.contains("subtle contrast, restrained saturation and a fine film-like grain."),
                "{g}"
            );
            assert!(
                p.contains("An authentic candid moment, photorealistic, understated."),
                "{g}"
            );
            assert!(p.contains("feels deliberately composed, with a muted neutral palette and slightly desaturated tones. A real person"), "{g}");
        }
    }

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
        let a = build_image_prompt(StyleGenre::Minimal, "회색 티, 데님", 12345);
        let b = build_image_prompt(StyleGenre::Minimal, "회색 티, 데님", 12345);
        assert_eq!(a, b);
    }

    /// 착장이 다르면 장소도 실제로 달라져야 한다.
    ///
    /// 프롬프트에 "매번 다른 곳을 고르라" 고 적는 것으로는 보장되지 않는다 —
    /// 이미지 모델은 이전 생성 결과를 모른다. 그래서 코드가 고르고, 그 선택이
    /// 정말 갈리는지 여기서 확인한다.
    #[test]
    fn different_outfits_reach_different_settings() {
        let hashes: Vec<u64> = (0..4000u64)
            .map(|h| h.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .collect();
        for &g in StyleGenre::ALL {
            let (own, _) = genre_setting(g);
            let seen: std::collections::HashSet<&str> =
                hashes.iter().map(|&h| pick_setting(g, h)).collect();
            for s in own.iter().chain(SHARED_SETTINGS.iter()) {
                assert!(seen.contains(s), "{g}: {s} 에 닿지 못한다");
            }
            // 무드 장소가 절반 안팎은 나온다 — 공통 장소에 묻히지 않는다.
            let own_share = hashes
                .iter()
                .filter(|&&h| own.contains(&pick_setting(g, h)))
                .count() as f64
                / hashes.len() as f64;
            assert!(own_share >= 0.45, "{g} 무드 장소 비중 {own_share}");
        }
        // 스포티 배경이 운동 장소로 굳지 않는다.
        let (sporty, _) = genre_setting(StyleGenre::SportyCasual);
        assert!(
            sporty
                .iter()
                .all(|s| !s.contains("track") && !s.contains("field"))
        );
    }

    /// 어느 무드든 밝은·그늘진·따뜻한 빛을 모두 쓴다 — 특정 무드가 좋은 빛을 독점하지 않는다.
    #[test]
    fn every_mood_reaches_every_shared_light() {
        let hashes: Vec<u64> = (0..2000u64)
            .map(|h| h.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .collect();
        for &g in StyleGenre::ALL {
            let seen: std::collections::HashSet<&str> =
                hashes.iter().map(|&h| pick_light(g, h)).collect();
            for l in SHARED_LIGHTS {
                assert!(seen.contains(l), "{g}: {l} 에 닿지 못한다");
            }
            assert!(!genre_backdrop(g).is_empty());
        }
        let p = build_image_prompt(StyleGenre::Minimal, "회색 티", 42);
        assert!(p.contains("the background brings generous empty space"));
    }

    /// 자세는 다섯 후보에 모두 닿고, 기대는 자세는 기댈 것이 있는 장소에서만 나온다.
    #[test]
    fn partial_edits_stay_inside_what_was_asked() {
        assert!(asks_to_modify_item(
            "라이트그레이 오버사이즈 맨투맨 정면에 스누피 그래픽 추가해줘"
        ));
        assert!(!asks_to_modify_item("로고 티로 바꿔줘"));
        assert!(!asks_to_modify_item("아우터 추가해줘"));

        assert_eq!(mentioned_categories("신발만 로퍼로 바꿔줘"), vec!["신발"]);
        assert_eq!(
            mentioned_categories("신발이랑 가방 바꿔줘"),
            vec!["신발", "가방"]
        );
        assert!(mentioned_categories("좀 더 화사하게 해줘").is_empty());

        let shoes = ["신발"];
        assert!(swap_allowed("신발", &shoes, &[], false));
        assert!(!swap_allowed("하의", &shoes, &[], false));
        // 카테고리를 말하지 않았으면 평가 뒤에는 이미 바꾼 칸만.
        let done = ["상의".to_string()];
        assert!(swap_allowed("상의", &[], &done, true));
        assert!(!swap_allowed("신발", &[], &done, true));
    }

    #[test]
    fn color_words_widen_to_wardrobe_names() {
        assert!(color_family("카멜색").contains(&"탄 "));
        assert!(color_family("검은 로퍼").contains(&"블랙"));
        assert!(color_family("로퍼").is_empty());
        assert_eq!(kind_family("로퍼로 바꿔줘"), vec!["로퍼"]);
        assert!(kind_family("카멜색").is_empty());
        // "탄" 은 "탄탄한" 같은 말에도 들어가므로 이름 쪽은 띄어쓰기까지 본다.
        assert!(!"탄탄한 스니커즈".contains(color_family("카멜")[1]));
    }

    #[test]
    fn edit_diff_splits_outside_material_brackets() {
        let base = "top/inner: 화이트 티셔츠 (cotton jersey, oversized), pants: 블랙 슬랙스 (polyester, wide leg), shoes: 화이트 스니커즈 (leather, clean low top)";
        let next = "top/inner: 화이트 티셔츠 (cotton jersey, oversized), pants: 블랙 슬랙스 (polyester, wide leg), shoes: 블랙 로퍼 (leather, penny)";
        assert_eq!(split_image_items(base).len(), 3);
        assert_eq!(
            diff_image_items(base, next),
            vec![ItemChange::Replace {
                slot: "shoes".into(),
                from: "화이트 스니커즈 (leather, clean low top)".into(),
                to: "블랙 로퍼 (leather, penny)".into(),
            }]
        );
        assert!(diff_image_items(base, base).is_empty());

        let with_coat = format!("{next}, outerwear: 베이지 코트 (wool)");
        let changes = diff_image_items(base, &with_coat);
        assert_eq!(changes.len(), 2);
        assert!(matches!(changes[1], ItemChange::Add { .. }));
        assert!(matches!(
            diff_image_items(&with_coat, next)[0],
            ItemChange::Remove { .. }
        ));
        let prompt = build_edit_prompt(&changes);
        assert!(prompt.contains("replace her shoes"));
        assert!(prompt.contains("as her outer layer"));
    }

    #[test]
    fn poses_vary_and_leaning_needs_a_wall() {
        let hashes: Vec<u64> = (0..2000u64)
            .map(|h| h.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .collect();
        let lean = POSES[POSES.len() - 1];
        let with_wall: std::collections::HashSet<&str> = hashes
            .iter()
            .map(|&h| pick_pose("a pale stucco wall", h))
            .collect();
        assert_eq!(with_wall.len(), POSES.len());
        assert!(
            hashes
                .iter()
                .all(|&h| pick_pose("a wide open plaza with pale paving", h) != lean)
        );
        let p = build_image_prompt(StyleGenre::Street, "회색 후드티", 99);
        assert!(!p.contains("not posing for the photographer"));
        assert!(p.contains("Her arms and bag never cover the main details of the top"));
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

    /// 헤어는 가중치대로 고르되 모든 후보에 닿고, 가중치 1인 개성 강한 헤어는 가끔만
    /// 나온다. 매니시의 긴 머리처럼 무드와 반대로 보이는 일상 헤어가 실제로 나와야 한다.
    #[test]
    fn hair_follows_weights_and_reaches_every_choice() {
        let hashes: Vec<u64> = (0..4000u64)
            .map(|h| h.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .collect();
        for &g in StyleGenre::ALL {
            let choices = genre_hair(g);
            assert!(choices.len() >= 2, "{g} 의 헤어 후보가 하나뿐");
            let total: u32 = choices.iter().map(|(_, w)| w).sum();
            for (hair, w) in choices {
                let share = hashes.iter().filter(|&&h| pick_hair(g, h) == *hair).count() as f64
                    / hashes.len() as f64;
                let want = *w as f64 / total as f64;
                assert!(
                    (share - want).abs() < 0.04,
                    "{g} {hair}: {share:.3} vs {want:.3}"
                );
            }
        }
        let mannish = genre_hair(StyleGenre::Mannish);
        assert!(mannish.iter().any(|(h, w)| h.contains("long") && *w >= 3));
        assert!(mannish.iter().any(|(h, w)| h.contains("pixie") && *w == 1));
    }

    /// 모델 출신은 착장마다 갈리고, 해시를 훑으면 목록 전부에 닿으며, 한국이 절반이다.
    #[test]
    fn model_origin_varies_and_keeps_korean_at_half() {
        let picks: Vec<&str> = (0..2000u64)
            .map(|h| model_origin(h.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
            .collect();
        let distinct: std::collections::HashSet<_> = picks.iter().collect();
        assert_eq!(distinct.len(), 8, "닿지 못하는 출신이 있다");
        let korean = picks.iter().filter(|o| **o == "Korean").count() as f64 / picks.len() as f64;
        assert!((0.4..0.6).contains(&korean), "한국 비중 {korean}");
        let p = build_image_prompt(StyleGenre::Minimal, "회색 티", 5u64 << 32);
        assert!(p.contains(&format!("a real adult {} woman", model_origin(5u64 << 32))));
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
