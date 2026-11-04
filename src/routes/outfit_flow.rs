//! 매일 쓰는 코디 흐름 API. 모두 본인 데이터만 다룬다 — 사용자는 인증에서 정해지고 본문으로 받지 않는다.
//!
//!   POST /api/outfits/replacement  {outfit_ref, category, replacement_item_id? | preference? , request_text?}
//!   POST /api/outfits/plan         {outfit_ref, date?, comment?, look_id?}
//!   POST /api/outfits/worn         {outfit_ref | record_id + expected_revision, date?, reaction?, comment?}
//!   POST /api/outfits/record       {record_id, expected_revision, action: cancel|undo|edit, ...}
//!   POST /api/outfits/history      {from?, to?, status?, limit?}
//!   POST /api/outfits/from-look    {look_id}  (웹 전용, 룩북 코디 → 참조)
//!   POST /api/outfits/detail       {record_id}  (웹 전용, 기록의 코디와 고정된 사진. 새로 만들지 않는다)
//!
//! 경로에 id 를 넣지 않는다 — MCP 위임은 고정 경로만 허용하고(`middleware::delegation`), id 의 소유자는
//! 여기서 언제나 인증된 사용자로 확인한다. 오류 본문은 `{error, code}` 이고 `error` 는 그대로 보여도 되는
//! 한국어 문장이다.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::AppState;
use crate::db::clothing_repo;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::models::outfit::{OutfitContext, OutfitSlot};
use crate::models::style_vocab::StyleGenre;
use crate::routes::recommendation::{current_season_label, feedback_bonus, load_feedback};
use crate::services::item_resolution::slot_of_category;
use crate::services::outfit_comparison::{ItemSnapshot, item_key};
use crate::services::outfit_flow::{
    self as flow, Edit, FlowError, HistoryQuery, NewRef, OutfitRef, Pin, Reaction, WearStatus,
    WornInput, WriteResult, today_kst,
};
use crate::services::outfit_replacement::{Intent, Preference, parse_intent};
use crate::services::serving_ranker::{WEATHER_BORDERLINE_PENALTY, compute_today_fit};
use crate::services::style_engine_v2::TodayFitLevel;
use crate::services::{mood_gate, mood_rules, shortlist, wear_history};

/// 돌려줄 대안 수.
const MAX_ALTERNATIVES: usize = 3;
/// 평가할 후보 수의 상한. 후보 하나에 질의 두 번이 든다.
const MAX_POOL: usize = 60;
/// 저장된 날씨를 믿는 시간. 이보다 오래된 코디는 지금 날씨로 다시 판단한다.
const WEATHER_FRESH_HOURS: i64 = 24;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/replacement", post(replace_item))
        .route("/plan", post(plan))
        .route("/worn", post(worn))
        .route("/record", post(update_record))
        .route("/history", post(history))
        .route("/from-look", post(from_look))
        .route("/detail", post(detail))
}

/// 흐름 실패의 HTTP 응답. DB 오류 내용은 로그에만 남긴다.
struct ApiError(FlowError);

impl From<FlowError> for ApiError {
    fn from(e: FlowError) -> Self {
        ApiError(e)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError(FlowError::Db(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use FlowError as F;
        let e = self.0;
        let status = match &e {
            F::RefNotFound | F::RecordNotFound | F::LookNotFound | F::ItemNotOwned => {
                StatusCode::NOT_FOUND
            }
            F::ItemGone
            | F::LegacyLook
            | F::InvalidTransition(_)
            | F::RevisionConflict(_)
            | F::DuplicateOnDate
            | F::LookMismatch => StatusCode::CONFLICT,
            F::Busy => StatusCode::SERVICE_UNAVAILABLE,
            F::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        if let F::Db(err) = &e {
            tracing::error!("코디 기록 DB 오류: {err}");
        }
        let mut body = json!({ "error": e.message(), "code": e.code() });
        if let F::RevisionConflict(current) = &e {
            body["current_revision"] = json!(current);
        }
        (status, Json(body)).into_response()
    }
}

fn parse_date(raw: Option<&str>) -> Result<Option<NaiveDate>, FlowError> {
    let Some(raw) = raw else { return Ok(None) };
    // parse_from_str 는 '2026-11-4' 도 받는다. 자릿수까지 맞아야 한다.
    if raw.len() != 10 {
        return Err(FlowError::InvalidDate);
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| FlowError::InvalidDate)
}

fn parse_reaction(raw: Option<&str>) -> Result<Option<Reaction>, FlowError> {
    match raw {
        None => Ok(None),
        Some(r) => Reaction::parse(r)
            .map(Some)
            .ok_or(FlowError::InvalidReaction),
    }
}

/// 값이 왔는지(`null` 포함)를 구분한다. `null` 은 지우라는 뜻이다.
fn some_value<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

// ─── 계획·착용 ───

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    outfit_ref: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    comment: Option<String>,
    /// 이 코디의 룩북 룩(웹). 소유자와 옷 구성을 확인한 뒤 기록에 고정한다.
    #[serde(default)]
    look_id: Option<String>,
}

/// 기록에 고정할 룩과 사진을 정한다. 룩북에서 만든 참조는 그 룩이고, 웹이 보낸 룩은 내 룩이면서 옷 id 구성이
/// 코디와 정확히 같아야 한다. 사진은 내 사진일 때만 고정한다.
async fn resolve_pin(
    state: &AppState,
    user_id: &str,
    outfit_ref: &str,
    requested: Option<&str>,
) -> Result<Option<Pin>, FlowError> {
    let outfit = flow::get_ref(&state.db, user_id, outfit_ref).await?;
    let id = match (outfit.look_id.as_deref(), requested) {
        (Some(a), Some(b)) if a != b => return Err(FlowError::LookMismatch),
        (a, b) => a.or(b),
    };
    let Some(id) = id else { return Ok(None) };
    let Some(look) = flow::get_look(&state.db, user_id, id).await? else {
        return match requested {
            Some(_) => Err(FlowError::LookNotFound),
            None => Ok(None),
        };
    };
    if requested.is_some() && !look.same_items(&outfit.item_key) {
        return Err(FlowError::LookMismatch);
    }
    let image_path = look_photo(&state.db, user_id, &look).await?;
    Ok(Some(Pin {
        look_id: look.id,
        image_path,
    }))
}

/// 이 룩의 내 사진. 룩에 붙은 사진, 없으면 같은 룩 문구의 사진 캐시(`list_looks` 와 같은 순서). 만들지 않는다.
async fn look_photo(
    db: &sqlx::MySqlPool,
    user_id: &str,
    look: &flow::OwnedLook,
) -> Result<Option<String>, FlowError> {
    let path = match &look.image_path {
        Some(p) => Some(p.clone()),
        None => {
            crate::routes::lookbook::cached_image_path(
                db,
                user_id,
                &look.image_prompt,
                look.mood_key.as_deref(),
            )
            .await
        }
    };
    let Some(path) = path else { return Ok(None) };
    let mine = flow::owned_image(db, user_id, &path).await?.is_some();
    Ok(mine.then_some(path))
}

async fn plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PlanRequest>,
) -> Result<Json<WriteResult>, ApiError> {
    let date = parse_date(body.date.as_deref())?;
    let pin = resolve_pin(
        &state,
        &auth.user_id,
        &body.outfit_ref,
        body.look_id.as_deref(),
    )
    .await?;
    let r = flow::plan(
        &state.db,
        &auth.user_id,
        &body.outfit_ref,
        date,
        body.comment.as_deref(),
        pin.as_ref(),
        today_kst(),
    )
    .await?;
    Ok(Json(r))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WornRequest {
    #[serde(default)]
    outfit_ref: Option<String>,
    #[serde(default)]
    record_id: Option<String>,
    #[serde(default)]
    expected_revision: Option<i32>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    reaction: Option<String>,
    #[serde(default)]
    comment: Option<String>,
}

async fn worn(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<WornRequest>,
) -> Result<Json<WriteResult>, ApiError> {
    let input = WornInput {
        date: parse_date(body.date.as_deref())?,
        reaction: parse_reaction(body.reaction.as_deref())?,
        comment: body.comment.as_deref(),
    };
    let today = today_kst();
    let r = match (&body.outfit_ref, &body.record_id, body.expected_revision) {
        (Some(r), None, None) => {
            let pin = resolve_pin(&state, &auth.user_id, r, None).await?;
            flow::worn_by_ref(&state.db, &auth.user_id, r, input, pin.as_ref(), today).await?
        }
        (None, Some(id), Some(rev)) => {
            flow::confirm_record(&state.db, &auth.user_id, id, rev, input, today).await?
        }
        _ => {
            return Err(FlowError::InvalidRequest(
                "outfit_ref 하나, 또는 record_id 와 expected_revision 을 함께 보내 주세요.",
            )
            .into());
        }
    };
    Ok(Json(r))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordRequest {
    record_id: String,
    expected_revision: i32,
    action: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default, deserialize_with = "some_value")]
    reaction: Option<Option<String>>,
    #[serde(default, deserialize_with = "some_value")]
    comment: Option<Option<String>>,
}

async fn update_record(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RecordRequest>,
) -> Result<Json<WriteResult>, ApiError> {
    let (db, user) = (&state.db, auth.user_id.as_str());
    let (id, rev) = (body.record_id.as_str(), body.expected_revision);
    let bare = body.date.is_none() && body.reaction.is_none() && body.comment.is_none();
    let r = match body.action.as_str() {
        "cancel" | "undo" if !bare => {
            return Err(FlowError::InvalidRequest(
                "취소·되돌리기에는 날짜나 메모를 함께 보내지 않아요.",
            )
            .into());
        }
        "cancel" => flow::cancel_record(db, user, id, rev).await?,
        "undo" => flow::undo_record(db, user, id, rev).await?,
        "edit" if bare => {
            return Err(FlowError::InvalidRequest("고칠 내용을 보내 주세요.").into());
        }
        "edit" => {
            let reaction = match &body.reaction {
                Some(r) => Some(parse_reaction(r.as_deref())?),
                None => None,
            };
            let edit = Edit {
                date: parse_date(body.date.as_deref())?,
                reaction,
                comment: body.comment.as_ref().map(|c| c.as_deref()),
            };
            flow::edit_record(db, user, id, rev, edit, today_kst()).await?
        }
        _ => {
            return Err(
                FlowError::InvalidRequest("action 은 cancel·undo·edit 중 하나예요.").into(),
            );
        }
    };
    Ok(Json(r))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRequest {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn history(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<HistoryRequest>,
) -> Result<Json<flow::HistoryPage>, ApiError> {
    let today = today_kst();
    let status = match body.status.as_deref() {
        None => None,
        Some(s) => Some(WearStatus::from_request(s).ok_or(FlowError::InvalidStatus)?),
    };
    let q = HistoryQuery {
        from: parse_date(body.from.as_deref())?.unwrap_or(today - chrono::Duration::days(30)),
        to: parse_date(body.to.as_deref())?
            .unwrap_or(today + chrono::Duration::days(flow::MAX_PLAN_AHEAD_DAYS)),
        status,
        limit: body.limit.unwrap_or(30),
    };
    Ok(Json(
        flow::history(&state.db, &auth.user_id, q, today).await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FromLookRequest {
    look_id: String,
}

async fn from_look(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<FromLookRequest>,
) -> Result<Json<Value>, ApiError> {
    let id = flow::ref_from_look(&state.db, &auth.user_id, &body.look_id).await?;
    Ok(Json(json!({ "outfit_ref": id })))
}

// ─── 기록의 코디 보기 ───

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailRequest {
    record_id: String,
}

/// 고정된 사진의 화면 값. 내 사진이 아니게 됐거나 파일이 없으면 경로를 내보내지 않는다.
async fn image_view(
    db: &sqlx::MySqlPool,
    user_id: &str,
    path: Option<String>,
    basis: Option<String>,
) -> Result<Value, FlowError> {
    let empty = |status: &str| json!({ "status": status, "file": null, "presentation": null, "basis": null });
    let Some(path) = path else {
        return Ok(empty("none"));
    };
    let Some(presentation) = flow::owned_image(db, user_id, &path).await? else {
        return Ok(empty("file_missing"));
    };
    let file = path
        .strip_prefix("/static/images/")
        .filter(|f| crate::routes::images::valid_name(f));
    let exists = match file {
        Some(f) => tokio::fs::symlink_metadata(std::path::Path::new("static/images").join(f))
            .await
            .is_ok_and(|m| m.is_file()),
        None => false,
    };
    Ok(if exists {
        json!({ "status": "available", "file": file, "presentation": presentation, "basis": basis })
    } else {
        empty("file_missing")
    })
}

/// 기록의 저장된 코디와 저장 때 고정한 룩·사진. 비슷한 룩을 찾지 않고, 모델·이미지 생성도 부르지 않는다.
/// 저장 때 사진이 없었으면 고정된 그 룩에 생긴 사진을 여기서 한 번 고정한다.
async fn detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<DetailRequest>,
) -> Result<Json<Value>, ApiError> {
    let (db, user_id) = (&state.db, auth.user_id.as_str());
    let origin = flow::record_origin(db, user_id, &body.record_id).await?;
    let record = &origin.record;

    let look_id = origin
        .pinned_look_id
        .as_deref()
        .or(origin.ref_look_id.as_deref());
    let look = match look_id {
        Some(id) => flow::get_look(db, user_id, id).await?,
        None => None,
    };
    let look_status = match (&look, look_id) {
        (Some(_), _) => "linked",
        (None, Some(_)) => "deleted",
        (None, None) => "none",
    };

    let (mut path, mut basis) = (origin.pinned_image_path.clone(), origin.photo_basis.clone());
    if path.is_none()
        && let (Some(pinned), Some(look)) = (&origin.pinned_look_id, &look)
        && let Some(found) = look_photo(db, user_id, look).await?
    {
        flow::adopt_look_photo(db, user_id, &record.record_id, pinned, &found).await?;
        path = Some(found);
        basis = Some("later".to_string());
    }
    let image = image_view(db, user_id, path, basis).await?;

    let ids: Vec<String> = record.items.iter().map(|i| i.id.clone()).collect();
    let owned = flow::owned_ids(db, user_id, &ids).await?;
    let removed: Vec<&String> = ids.iter().filter(|id| !owned.contains(*id)).collect();

    Ok(Json(json!({
        "record": record,
        "source": origin.source,
        "look": look.as_ref().map(|l| json!({ "id": l.id, "title": l.title })),
        "look_status": look_status,
        "image": image,
        "removed_item_ids": removed,
    })))
}

// ─── 한 벌 바꾸기 ───

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceRequest {
    outfit_ref: String,
    category: String,
    #[serde(default)]
    replacement_item_id: Option<String>,
    #[serde(default)]
    preference: Option<String>,
    /// 사용자가 말한 조건 원문. 알아들을 수 없으면 되묻는다.
    #[serde(default)]
    request_text: Option<String>,
}

/// 판단에 쓴 기온과 그 출처.
struct WeatherBasis {
    temperature: f64,
    source: &'static str,
    at: String,
}

/// 코디에 저장된 날씨가 최근이면 그것을, 아니면 지금 날씨를 쓴다.
async fn weather_basis(
    state: &AppState,
    user_id: &str,
    outfit: &OutfitRef,
) -> Option<WeatherBasis> {
    let at = outfit
        .context
        .get("weather_at")
        .and_then(Value::as_str)
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok())
        .unwrap_or(outfit.created_at);
    let now = chrono::Local::now().naive_local();
    if let Some(t) = wear_history::context_temperature(&outfit.context)
        && (now - at).num_hours() < WEATHER_FRESH_HOURS
    {
        return Some(WeatherBasis {
            temperature: t,
            source: "recorded",
            at: at.format("%Y-%m-%d %H:%M:%S").to_string(),
        });
    }
    let region = crate::db::region_repo::get_region(&state.db, user_id)
        .await
        .ok()
        .flatten()?;
    let weather = crate::services::weather::fetch_weather(
        &state.http_client,
        &state.kma_api_key,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(|e| tracing::warn!("바꾸기 날씨 조회 실패: {e}"))
    .ok()?;
    Some(WeatherBasis {
        temperature: weather.temperature,
        source: "current",
        at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
    })
}

/// 평가를 통과한 대안 하나.
struct Scored<'a> {
    item: &'a Clothing,
    rank: i32,
    score: i32,
    borderline: bool,
    preference_fact: Option<String>,
}

fn attribute_line(c: &Clothing) -> Option<String> {
    let parts: Vec<&str> = [
        c.sub_category.as_deref(),
        c.color.as_deref(),
        c.material_primary.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect();
    (!parts.is_empty()).then(|| format!("옷장 정보: {}.", parts.join(" · ")))
}

fn clarification(outfit: &OutfitRef, category: &str, reason: &str, message: String) -> Json<Value> {
    Json(json!({
        "outcome": "needs_clarification",
        "reason": reason,
        "message": message,
        "outfit_ref": outfit.id,
        "category": category,
        "supported_preferences": Preference::ALL.map(Preference::key),
        "alternatives": [],
    }))
}

async fn slot_of(
    db: &sqlx::MySqlPool,
    item: &Clothing,
    kind: crate::models::outfit::SlotKind,
) -> OutfitSlot {
    OutfitSlot {
        slot: kind,
        clothing: item.clone(),
        seasons: clothing_repo::get_seasons(db, &item.id)
            .await
            .unwrap_or_default(),
        texture_worlds: clothing_repo::get_texture_worlds(db, &item.id)
            .await
            .unwrap_or_default(),
    }
}

async fn replace_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ReplaceRequest>,
) -> Result<Json<Value>, ApiError> {
    let user_id = auth.user_id.as_str();
    let kind = slot_of_category(&body.category).ok_or(FlowError::InvalidCategory)?;
    let category = body.category.trim().to_string();
    let outfit = flow::get_ref(&state.db, user_id, &body.outfit_ref).await?;

    let intent = parse_intent(
        &category,
        body.replacement_item_id.as_deref(),
        body.preference.as_deref(),
        body.request_text.as_deref(),
    );
    if let Intent::Clarify { reason, message } = &intent {
        return Ok(clarification(&outfit, &category, reason, message.clone()));
    }

    let targets: Vec<&ItemSnapshot> = outfit
        .items
        .iter()
        .filter(|i| i.category == category)
        .collect();
    let target = match targets.as_slice() {
        [t] => *t,
        [] => {
            return Ok(clarification(
                &outfit,
                &category,
                "slot_empty",
                format!("이 코디에는 {category} 자리가 없어요. 바꿀 자리를 다시 골라 주세요."),
            ));
        }
        _ => {
            return Ok(clarification(
                &outfit,
                &category,
                "slot_ambiguous",
                format!("이 코디에 {category}이(가) 둘 이상이라 어느 것을 바꿀지 알 수 없어요."),
            ));
        }
    };

    // 코디의 옷이 모두 지금도 내 옷이어야 한다. 바꾸지 않는 자리도 값 그대로 다시 평가한다.
    let clothes = clothing_repo::list_clothing(&state.db, user_id).await?;
    let find = |id: &str| clothes.iter().find(|c| c.id == id);
    let mut base_slots = Vec::with_capacity(outfit.items.len());
    for item in outfit.items.iter().filter(|i| i.id != target.id) {
        let c = find(&item.id).ok_or(FlowError::ItemGone)?;
        let k = slot_of_category(&c.category).ok_or(FlowError::ItemGone)?;
        base_slots.push(slot_of(&state.db, c, k).await);
    }
    let current = find(&target.id).ok_or(FlowError::ItemGone)?;

    let situation = outfit
        .context
        .get("occasion")
        .and_then(Value::as_str)
        .map(str::to_string);
    // 바꾸는 자리에 `slot` 을 넣은 구성.
    let context = |slot: OutfitSlot| OutfitContext {
        slots: std::iter::once(slot)
            .chain(base_slots.iter().cloned())
            .collect(),
        situation: situation.clone(),
    };

    let Some(basis) = weather_basis(&state, user_id, &outfit).await else {
        return Ok(Json(json!({
            "outcome": "weather_unavailable",
            "message": "날씨를 확인하지 못해 후보를 판단하지 않았어요. 지역 설정을 확인하거나 잠시 뒤 다시 시도해 주세요.",
            "outfit_ref": outfit.id,
            "category": category,
            "alternatives": [],
        })));
    };
    let temperature = basis.temperature;
    let mood = outfit
        .context
        .get("mood_key")
        .and_then(Value::as_str)
        .and_then(StyleGenre::from_alias);
    let gender = outfit.context.get("gender").and_then(Value::as_str);
    let season = current_season_label();
    let today = today_kst();
    let recent = wear_history::load_recent(&state.db, user_id, today)
        .await
        .unwrap_or_default();
    let feedback = load_feedback(&state.db, user_id).await;

    let (explicit, preference) = match &intent {
        Intent::Explicit(id) => (Some(id.as_str()), None),
        Intent::Find(p) => (None, *p),
        Intent::Clarify { .. } => unreachable!("위에서 돌려보냈다"),
    };
    let in_outfit = |id: &str| outfit.items.iter().any(|i| i.id == id);

    let pool: Vec<&Clothing> = if let Some(id) = explicit {
        let c = find(id).ok_or(FlowError::ItemNotOwned)?;
        if c.category != category {
            return Err(FlowError::WrongCategory.into());
        }
        if in_outfit(id) {
            return Err(FlowError::SameItem.into());
        }
        vec![c]
    } else {
        clothes
            .iter()
            .filter(|c| c.category == category && !in_outfit(&c.id))
            .filter(|c| gender.is_none() || c.gender.as_deref() == gender)
            .filter(|c| shortlist::is_temp_appropriate(c, temperature))
            .filter(|c| !mood_gate::hidden_on_mild_pop_day(c, mood, Some(temperature)))
            .filter(|c| preference.is_none_or(|p| p.check(c, current).is_some()))
            .take(MAX_POOL)
            .collect()
    };

    let original_score = mood_rules::evaluate_for_mood(
        &context(slot_of(&state.db, current, kind).await),
        season.as_deref(),
        mood,
    )
    .score;

    let mut unsuitable = 0usize;
    let mut scored: Vec<Scored<'_>> = Vec::new();
    let keep: Vec<&Clothing> = base_slots.iter().map(|s| &s.clothing).collect();
    for c in pool {
        let ctx = context(slot_of(&state.db, c, kind).await);
        let fit = compute_today_fit(&ctx, temperature);
        if fit == TodayFitLevel::Fail || !shortlist::is_temp_appropriate(c, temperature) {
            unsuitable += 1;
            continue;
        }
        let items: Vec<&Clothing> = keep.iter().copied().chain([c]).collect();
        let score = mood_rules::evaluate_for_mood(&ctx, season.as_deref(), mood).score;
        let borderline = fit == TodayFitLevel::Borderline;
        let worn: Vec<wear_history::WornItem<'_>> = items
            .iter()
            .map(|i| (i.id.as_str(), i.category.as_str()))
            .collect();
        let rank = score + feedback_bonus(&items, &feedback)
            - if borderline {
                WEATHER_BORDERLINE_PENALTY
            } else {
                0
            }
            - wear_history::raw_penalty(&worn, &recent, today);
        scored.push(Scored {
            item: c,
            rank,
            score,
            borderline,
            preference_fact: preference.and_then(|p| p.check(c, current)),
        });
    }

    // 무드를 읽을 수 없는 후보를 뺀다. 전부 그렇다면 남겨 두고 알린다(추천과 같은 기준).
    let mut notice = None;
    if explicit.is_none()
        && let Some(m) = mood
    {
        let reads = |s: &Scored<'_>| {
            let items: Vec<&Clothing> = keep.iter().copied().chain([s.item]).collect();
            mood_gate::reads_as_at(m, &items, Some(temperature)) != Some(false)
        };
        if scored.iter().any(reads) {
            scored.retain(reads);
        } else if !scored.is_empty() {
            notice = Some("이 무드를 살리는 후보가 없어 기본에 가까운 옷만 남았어요.".to_string());
        }
    }

    scored.sort_by(|a, b| b.rank.cmp(&a.rank).then_with(|| a.item.id.cmp(&b.item.id)));
    scored.truncate(MAX_ALTERNATIVES);

    if scored.is_empty() {
        let message = if explicit.is_some() {
            format!(
                "고른 옷은 {temperature}°C 기준 입기 판정을 통과하지 못해 바꾸지 않았어요. 다른 옷을 골라 주세요."
            )
        } else if preference.is_some() {
            "조건에 맞으면서 오늘 입을 수 있는 옷이 옷장에 없어요. 조건을 바꿔 보세요.".to_string()
        } else {
            "같은 카테고리에서 오늘 입을 수 있는 다른 옷이 없어요.".to_string()
        };
        return Ok(Json(json!({
            "outcome": "no_candidates",
            "message": message,
            "outfit_ref": outfit.id,
            "category": category,
            "excluded_by_weather": unsuitable,
            "alternatives": [],
        })));
    }

    let mut alternatives = Vec::with_capacity(scored.len());
    for s in &scored {
        // 바꾸는 자리 말고는 저장된 스냅숏을 그대로 쓴다. 순서도 유지한다.
        let items: Vec<ItemSnapshot> = outfit
            .items
            .iter()
            .map(|i| {
                if i.id == target.id {
                    flow::snapshot_of(s.item)
                } else {
                    i.clone()
                }
            })
            .collect();
        let mut new_context = outfit.context.clone();
        if let Some(obj) = new_context.as_object_mut() {
            obj.insert("replaced_from".into(), json!(target.id));
            obj.insert("weather_at".into(), json!(basis.at));
            let weather = obj.entry("weather").or_insert_with(|| json!({}));
            if let Some(w) = weather.as_object_mut() {
                w.insert("temperature".into(), json!(temperature));
            }
        }
        let new_ref = flow::save_ref(
            &state.db,
            user_id,
            NewRef {
                source: "replacement",
                dedupe_key: format!("replace:{}:{}", outfit.id, s.item.id),
                batch_id: None,
                mode: None,
                parent_ref_id: Some(&outfit.id),
                replaced_category: Some(&category),
                items: &items,
                context: new_context,
            },
        )
        .await?;

        let mut rationale: Vec<String> = Vec::new();
        rationale.extend(s.preference_fact.clone());
        rationale.extend(attribute_line(s.item));
        rationale.push(if s.borderline {
            format!("{temperature}°C 기준 입을 수는 있지만 경계예요.")
        } else {
            format!("{temperature}°C 기준 입기 판정을 통과했어요.")
        });
        rationale.push(format!(
            "조합 평가(내부 순위 점수)는 {original_score} → {} 예요.",
            s.score
        ));
        rationale.extend(wear_history::wear_note(
            &[s.item.id.as_str()],
            &recent,
            today,
            0,
        ));
        alternatives.push(json!({
            "outfit_ref": new_ref,
            "new_item": flow::snapshot_of(s.item),
            "items": items,
            "item_key": item_key(items.iter().map(|i| i.id.as_str())),
            "rationale": rationale,
            "weather_fit": if s.borderline { "borderline" } else { "ok" },
        }));
    }

    let mut limitations = vec![
        "내 옷장의 같은 카테고리 옷만 봤어요. 나머지 옷은 그대로예요.".to_string(),
        "제안일 뿐이에요. 원래 코디와 기록은 바뀌지 않았어요.".to_string(),
    ];
    if preference == Some(Preference::Comfortable) {
        limitations.push("편안함은 신발 종류로만 판단했어요(착화감 데이터 없음).".to_string());
    }
    if basis.source == "current" {
        limitations.push("코디가 오래돼 지금 날씨로 다시 판단했어요.".to_string());
    }

    Ok(Json(json!({
        "outcome": "proposed",
        "message": format!("{category} 대안 {}개", alternatives.len()),
        "outfit_ref": outfit.id,
        "category": category,
        "replaced_item": target,
        "preference": preference.map(Preference::key),
        "weather": { "temperature": temperature, "source": basis.source, "at": basis.at },
        "notice": notice,
        "alternatives": alternatives,
        "limitations": limitations,
    })))
}
