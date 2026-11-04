//! 매일 쓰는 코디 흐름의 저장소(마이그레이션 `20261104000001`).
//!
//! 흐름: 추천(`outfit_ref` 가 생긴다) → 필요하면 한 벌 바꾸기(새 참조, `outfit_replacement`) →
//! 입을 계획(`planned`) → 실제로 입음(`worn`). 계획은 착용이 아니고, 착용은 사용자가 직접 말했을 때만
//! 기록한다. 잘못 남긴 착용은 되돌릴 수 있다.
//!
//! 상태 전이:
//!   planned → worn       확정. 날짜를 바꿀 수 있다(미래는 안 된다).
//!   planned → cancelled  취소. 행은 남고 이력·감점에서 빠진다.
//!   worn → planned       되돌리기(계획에서 온 기록). 반응을 지운다.
//!   worn → cancelled     되돌리기(계획 없이 바로 남긴 기록).
//! cancelled 는 끝이다. 다시 입으려면 같은 참조로 새로 계획한다.
//!
//! 사진: 저장할 때 정확한 룩 id 와 그때의 사진 경로를 기록에 고정한다(`Pin`). 조회는 이 값만 쓰고 비슷한 룩을
//! 찾지 않는다. 저장 때 사진이 없었으면 고정된 그 룩에 처음 생긴 사진만 한 번 이어 받는다(`adopt_look_photo`).
//! 확정·수정·되돌리기는 이 값을 건드리지 않고, 재요청은 먼저 고정한 것을 바꾸지 못한다.
//!
//! 동시성: 쓰기는 모두 조건 있는 한 문장(`WHERE status = ? AND revision = ?`)이거나 유일 키에 기대는
//! INSERT 라 트랜잭션 없이도 원자적이다. 지면 다시 읽어 결과를 가른다 — 같은 요청의 재시도는
//! `already_*` 로 끝나고 행이 늘지 않는다.

use std::collections::HashSet;

use chrono::{Duration, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::MySqlPool;
use sqlx::types::Json;

use crate::models::clothing::Clothing;
use crate::services::outfit_comparison::{BatchContext, CandidateSnapshot, ItemSnapshot, item_key};

/// 계획은 앞으로 이만큼까지.
pub const MAX_PLAN_AHEAD_DAYS: i64 = 60;
/// 입은 날짜는 뒤로 이만큼까지.
pub const MAX_WORN_BACKDATE_DAYS: i64 = 365;
/// 메모 길이(글자 수). 표의 `comment VARCHAR(200)` 과 같다.
pub const COMMENT_MAX_CHARS: usize = 200;
/// 기록 조회 한 번의 최대 행 수와 날짜 범위.
pub const HISTORY_MAX_LIMIT: i64 = 100;
pub const HISTORY_MAX_RANGE_DAYS: i64 = 500;
/// 경합에서 졌을 때 다시 읽는 횟수.
const MAX_ATTEMPTS: usize = 6;

/// 서울 날짜. 한국은 서머타임이 없어 고정 오프셋으로 충분하다.
pub fn today_kst() -> NaiveDate {
    let kst = FixedOffset::east_opt(9 * 3600).expect("유효한 오프셋");
    Utc::now().with_timezone(&kst).date_naive()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WearStatus {
    Planned,
    Worn,
    Cancelled,
}

impl WearStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WearStatus::Planned => "planned",
            WearStatus::Worn => "worn",
            WearStatus::Cancelled => "cancelled",
        }
    }

    /// 요청 값 해석. 저장된 값은 [`Self::parse`] 로 읽는다.
    pub fn from_request(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(WearStatus::Planned),
            "worn" => Some(WearStatus::Worn),
            "cancelled" => Some(WearStatus::Cancelled),
            _ => None,
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "worn" => WearStatus::Worn,
            "cancelled" => WearStatus::Cancelled,
            _ => WearStatus::Planned,
        }
    }
}

/// 입어 본 뒤의 한 단어 반응.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    Comfortable,
    Uncomfortable,
    TooCold,
    TooHot,
    Liked,
    Disliked,
}

impl Reaction {
    pub fn parse(s: &str) -> Option<Self> {
        [
            Reaction::Comfortable,
            Reaction::Uncomfortable,
            Reaction::TooCold,
            Reaction::TooHot,
            Reaction::Liked,
            Reaction::Disliked,
        ]
        .into_iter()
        .find(|r| r.as_str() == s)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Reaction::Comfortable => "comfortable",
            Reaction::Uncomfortable => "uncomfortable",
            Reaction::TooCold => "too_cold",
            Reaction::TooHot => "too_hot",
            Reaction::Liked => "liked",
            Reaction::Disliked => "disliked",
        }
    }
}

/// 흐름의 실패. 화면·MCP 는 `code` 로 가르고 `message` 는 그대로 사용자에게 보여도 된다.
#[derive(Debug)]
pub enum FlowError {
    /// 없거나 다른 계정의 참조. 둘을 구별해 주지 않는다.
    RefNotFound,
    RecordNotFound,
    LookNotFound,
    /// 룩에 옷 id 가 없다(id 를 쓰기 전에 저장한 룩).
    LegacyLook,
    /// 코디의 옷 중 지금 내 옷장에 없는 것이 있다.
    ItemGone,
    /// 바꿀 옷이 내 옷장에 없다. 다른 계정의 옷이어도 같은 답이다.
    ItemNotOwned,
    /// 바꿀 옷이 고른 자리의 카테고리가 아니다.
    WrongCategory,
    /// 지금 입은 옷과 같은 옷으로 바꾸려 했다.
    SameItem,
    /// 코디 자리 이름이 다섯 카테고리가 아니다.
    InvalidCategory,
    InvalidDate,
    InvalidReaction,
    InvalidStatus,
    /// 요청 모양이 맞지 않는다(둘 중 하나만 보내야 하는 값 등).
    InvalidRequest(&'static str),
    DateInFuture,
    DateInPast,
    DateTooFar,
    DateTooOld,
    InvalidRange,
    CommentTooLong,
    /// 반응은 입은 기록에만 남긴다.
    ReactionNeedsWorn,
    /// 지금 상태에서 할 수 없는 일.
    InvalidTransition(WearStatus),
    /// 그사이 기록이 바뀌었다. 최신 revision 을 알려 준다.
    RevisionConflict(i32),
    /// 그 날짜에 같은 구성의 기록이 이미 있다.
    DuplicateOnDate,
    /// 이 룩은 코디와 옷 구성이 다르다.
    LookMismatch,
    /// 경합이 풀리지 않았다. 바로 다시 시도해도 된다.
    Busy,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for FlowError {
    fn from(e: sqlx::Error) -> Self {
        FlowError::Db(e)
    }
}

impl FlowError {
    pub fn code(&self) -> &'static str {
        match self {
            FlowError::RefNotFound => "ref_not_found",
            FlowError::RecordNotFound => "record_not_found",
            FlowError::LookNotFound => "look_not_found",
            FlowError::LegacyLook => "legacy_look",
            FlowError::ItemGone => "item_gone",
            FlowError::ItemNotOwned => "item_not_owned",
            FlowError::WrongCategory => "wrong_category",
            FlowError::SameItem => "same_item",
            FlowError::InvalidCategory => "invalid_category",
            FlowError::InvalidDate => "invalid_date",
            FlowError::InvalidReaction => "invalid_reaction",
            FlowError::InvalidStatus => "invalid_status",
            FlowError::InvalidRequest(_) => "invalid_request",
            FlowError::DateInFuture => "date_in_future",
            FlowError::DateInPast => "date_in_past",
            FlowError::DateTooFar => "date_too_far",
            FlowError::DateTooOld => "date_too_old",
            FlowError::InvalidRange => "invalid_range",
            FlowError::CommentTooLong => "comment_too_long",
            FlowError::ReactionNeedsWorn => "reaction_needs_worn",
            FlowError::InvalidTransition(_) => "invalid_transition",
            FlowError::RevisionConflict(_) => "revision_conflict",
            FlowError::DuplicateOnDate => "duplicate_on_date",
            FlowError::LookMismatch => "look_mismatch",
            FlowError::Busy => "busy",
            FlowError::Db(_) => "internal",
        }
    }

    pub fn message(&self) -> String {
        match self {
            FlowError::RefNotFound => {
                "이 코디 참조를 찾을 수 없어요. 추천이나 바꾸기 결과에 나온 참조를 그대로 써 주세요.".into()
            }
            FlowError::RecordNotFound => "이 기록을 찾을 수 없어요.".into(),
            FlowError::LookNotFound => "이 코디를 찾을 수 없어요.".into(),
            FlowError::LegacyLook => {
                "예전에 저장한 코디라 옷 정보를 확인할 수 없어요. 기록으로 남기지 않았어요.".into()
            }
            FlowError::ItemGone => {
                "이 코디의 옷 중 지금 옷장에 없는 것이 있어 기록하지 않았어요.".into()
            }
            FlowError::ItemNotOwned => "내 옷장에서 이 옷을 찾을 수 없어요.".into(),
            FlowError::WrongCategory => "이 옷은 고른 자리의 카테고리가 아니에요.".into(),
            FlowError::SameItem => "지금 코디에 들어 있는 옷이에요. 다른 옷을 골라 주세요.".into(),
            FlowError::InvalidCategory => {
                "카테고리는 상의·하의·아우터·신발·가방 중 하나로 골라 주세요.".into()
            }
            FlowError::InvalidDate => "날짜는 YYYY-MM-DD 형식의 실제 날짜로 적어 주세요.".into(),
            FlowError::InvalidReaction => "지원하지 않는 반응이에요.".into(),
            FlowError::InvalidStatus => "상태는 planned·worn·cancelled 중 하나예요.".into(),
            FlowError::InvalidRequest(m) => (*m).to_string(),
            FlowError::DateInFuture => "아직 오지 않은 날짜에는 입었다고 기록할 수 없어요.".into(),
            FlowError::DateInPast => "지난 날짜에는 입을 계획을 세울 수 없어요.".into(),
            FlowError::DateTooFar => {
                format!("입을 계획은 {MAX_PLAN_AHEAD_DAYS}일 뒤까지만 세울 수 있어요.")
            }
            FlowError::DateTooOld => {
                format!("{MAX_WORN_BACKDATE_DAYS}일보다 오래된 날짜는 기록할 수 없어요.")
            }
            FlowError::InvalidRange => "조회 날짜 범위가 올바르지 않아요.".into(),
            FlowError::CommentTooLong => {
                format!("메모는 {COMMENT_MAX_CHARS}자 이내로 적어 주세요.")
            }
            FlowError::ReactionNeedsWorn => "반응은 입은 기록에만 남길 수 있어요.".into(),
            FlowError::InvalidTransition(s) => match s {
                WearStatus::Planned => "아직 계획 상태인 기록이라 할 수 없는 일이에요.".into(),
                WearStatus::Worn => {
                    "이미 입은 기록이에요. 잘못 남겼다면 되돌리기를 써 주세요.".into()
                }
                WearStatus::Cancelled => {
                    "취소된 기록이에요. 다시 입으려면 새로 계획해 주세요.".into()
                }
            },
            FlowError::RevisionConflict(_) => {
                "그사이 기록이 바뀌었어요. 최신 기록을 다시 확인해 주세요.".into()
            }
            FlowError::DuplicateOnDate => "그 날짜에 같은 코디 기록이 이미 있어요.".into(),
            FlowError::LookMismatch => "이 룩은 고른 코디와 옷 구성이 달라요. 기록하지 않았어요.".into(),
            FlowError::Busy => "기록이 동시에 바뀌고 있어요. 잠시 뒤 다시 시도해 주세요.".into(),
            FlowError::Db(_) => "기록을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.".into(),
        }
    }
}

/// 메모를 다듬는다. 줄바꿈 말고 제어 문자는 버린다. 비면 `None`.
pub fn clean_comment(raw: Option<&str>) -> Result<Option<String>, FlowError> {
    let Some(raw) = raw else { return Ok(None) };
    let cleaned: String = raw
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() > COMMENT_MAX_CHARS {
        return Err(FlowError::CommentTooLong);
    }
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

/// 입을 계획의 날짜 범위.
pub fn check_plan_date(date: NaiveDate, today: NaiveDate) -> Result<(), FlowError> {
    if date < today {
        return Err(FlowError::DateInPast);
    }
    if date > today + Duration::days(MAX_PLAN_AHEAD_DAYS) {
        return Err(FlowError::DateTooFar);
    }
    Ok(())
}

/// 입은 날짜의 범위.
pub fn check_worn_date(date: NaiveDate, today: NaiveDate) -> Result<(), FlowError> {
    if date > today {
        return Err(FlowError::DateInFuture);
    }
    if date < today - Duration::days(MAX_WORN_BACKDATE_DAYS) {
        return Err(FlowError::DateTooOld);
    }
    Ok(())
}

// ─── 참조 ───

/// 서버가 저장한 코디. 모든 후속 요청의 근거다.
#[derive(Debug, Clone)]
pub struct OutfitRef {
    pub id: String,
    pub item_key: String,
    pub items: Vec<ItemSnapshot>,
    pub context: Value,
    pub created_at: NaiveDateTime,
    /// 룩북에서 만든 참조의 룩 id. 다른 출처는 `None`.
    pub look_id: Option<String>,
}

/// 룩북에서 만든 참조의 `dedupe_key` 에서 룩 id 를 꺼낸다.
fn look_of(source: &str, dedupe_key: &str) -> Option<String> {
    (source == "lookbook")
        .then(|| dedupe_key.strip_prefix("look:").map(str::to_string))
        .flatten()
}

pub struct NewRef<'a> {
    pub source: &'a str,
    pub dedupe_key: String,
    pub batch_id: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub parent_ref_id: Option<&'a str>,
    pub replaced_category: Option<&'a str>,
    pub items: &'a [ItemSnapshot],
    pub context: Value,
}

/// 참조를 남기고 id 를 돌려준다. 같은 `dedupe_key` 면 처음 만든 것을 그대로 돌려준다.
pub async fn save_ref(
    db: &MySqlPool,
    user_id: &str,
    new: NewRef<'_>,
) -> Result<String, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let key = item_key(new.items.iter().map(|i| i.id.as_str()));
    sqlx::query(
        "INSERT INTO outfit_ref \
         (id, user_id, source, dedupe_key, batch_id, mode, parent_ref_id, replaced_category, \
          item_key, items_json, context_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE id = id",
    )
    .bind(&id)
    .bind(user_id)
    .bind(new.source)
    .bind(&new.dedupe_key)
    .bind(new.batch_id)
    .bind(new.mode)
    .bind(new.parent_ref_id)
    .bind(new.replaced_category)
    .bind(&key)
    .bind(Json(new.items))
    .bind(Json(&new.context))
    .execute(db)
    .await?;
    sqlx::query_scalar("SELECT id FROM outfit_ref WHERE user_id = ? AND dedupe_key = ?")
        .bind(user_id)
        .bind(&new.dedupe_key)
        .fetch_one(db)
        .await
}

/// 내 참조. 다른 계정의 것이면 없는 것과 같다.
pub async fn get_ref(db: &MySqlPool, user_id: &str, ref_id: &str) -> Result<OutfitRef, FlowError> {
    if ref_id.is_empty() || ref_id.len() > 36 {
        return Err(FlowError::RefNotFound);
    }
    type Row = (
        String,
        String,
        String,
        String,
        Json<Vec<ItemSnapshot>>,
        Json<Value>,
        NaiveDateTime,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, source, dedupe_key, item_key, items_json, context_json, created_at \
         FROM outfit_ref WHERE id = ? AND user_id = ?",
    )
    .bind(ref_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let (id, source, dedupe_key, item_key, items, context, created_at) =
        row.ok_or(FlowError::RefNotFound)?;
    Ok(OutfitRef {
        id,
        item_key,
        items: items.0,
        context: context.0,
        created_at,
        look_id: look_of(&source, &dedupe_key),
    })
}

/// 추천 묶음의 코디마다 참조를 만든다. 결과는 `candidates` 와 같은 순서다.
pub async fn save_batch_refs(
    db: &MySqlPool,
    user_id: &str,
    batch_id: &str,
    ctx: &BatchContext<'_>,
    candidates: &[CandidateSnapshot],
) -> Result<Vec<Option<String>>, sqlx::Error> {
    let mut refs = Vec::with_capacity(candidates.len());
    for c in candidates {
        if c.items.is_empty() {
            refs.push(None);
            continue;
        }
        let id = save_ref(
            db,
            user_id,
            NewRef {
                source: "recommendation",
                dedupe_key: format!("batch:{batch_id}:{}", c.position),
                batch_id: Some(batch_id),
                mode: Some(&c.mode),
                parent_ref_id: None,
                replaced_category: None,
                items: &c.items,
                context: json!({
                    "occasion": ctx.occasion,
                    "mood_key": ctx.mood_key,
                    "gender": ctx.gender,
                    "mode": c.mode,
                    "weather": ctx.weather,
                }),
            },
        )
        .await?;
        refs.push(Some(id));
    }
    Ok(refs)
}

/// 옷장 행을 스냅숏으로. 이름·카테고리는 언제나 서버의 값이다.
pub fn snapshot_of(c: &Clothing) -> ItemSnapshot {
    ItemSnapshot {
        id: c.id.clone(),
        name: c.name.clone(),
        category: c.category.clone(),
        color: c.color.clone(),
        sub_category: c.sub_category.clone(),
        material: c.image_material(),
        has_image: c.image_url.as_deref().is_some_and(|u| !u.is_empty()),
    }
}

/// 이 id 들 가운데 지금 이 사용자의 옷인 것.
pub async fn owned_ids(
    db: &MySqlPool,
    user_id: &str,
    ids: &[String],
) -> Result<HashSet<String>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT id FROM clothing WHERE user_id = ? AND id IN ({marks})");
    let mut q = sqlx::query_scalar::<_, String>(&sql).bind(user_id);
    for id in ids {
        q = q.bind(id);
    }
    Ok(q.fetch_all(db).await?.into_iter().collect())
}

async fn ensure_items_owned(
    db: &MySqlPool,
    user_id: &str,
    items: &[ItemSnapshot],
) -> Result<(), FlowError> {
    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let owned = owned_ids(db, user_id, &ids).await?;
    if ids.iter().all(|id| owned.contains(id)) {
        Ok(())
    } else {
        Err(FlowError::ItemGone)
    }
}

/// 내 룩북 코디의 참조. 룩에 적힌 이름·카테고리는 믿지 않고 옷 id 로 옷장에서 다시 읽는다.
pub async fn ref_from_look(
    db: &MySqlPool,
    user_id: &str,
    look_id: &str,
) -> Result<String, FlowError> {
    if look_id.is_empty() || look_id.len() > 36 {
        return Err(FlowError::LookNotFound);
    }
    let row: Option<(Json<Value>, Option<String>, String)> = sqlx::query_as(
        "SELECT outfit_json, mood_key, title FROM lookbook_look WHERE id = ? AND user_id = ?",
    )
    .bind(look_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let (outfit, mood_key, title) = row.ok_or(FlowError::LookNotFound)?;
    let ids: Option<Vec<String>> = outfit.0.as_array().map(|items| {
        items
            .iter()
            .filter_map(|i| i.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    });
    let count = outfit.0.as_array().map_or(0, Vec::len);
    let ids = ids.filter(|v| !v.is_empty() && v.len() == count);
    let Some(ids) = ids else {
        return Err(FlowError::LegacyLook);
    };
    let clothes = crate::db::clothing_repo::list_clothing(db, user_id).await?;
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        let c = clothes
            .iter()
            .find(|c| &c.id == id)
            .ok_or(FlowError::ItemGone)?;
        items.push(snapshot_of(c));
    }
    Ok(save_ref(
        db,
        user_id,
        NewRef {
            source: "lookbook",
            dedupe_key: format!("look:{look_id}"),
            batch_id: None,
            mode: None,
            parent_ref_id: None,
            replaced_category: None,
            items: &items,
            context: json!({ "mood_key": mood_key, "title": title }),
        },
    )
    .await?)
}

// ─── 기록 ───

#[derive(Debug, sqlx::FromRow)]
struct WearRow {
    id: String,
    outfit_ref_id: String,
    status: String,
    wear_date: NaiveDate,
    items_json: Json<Vec<ItemSnapshot>>,
    context_json: Json<Value>,
    reaction: Option<String>,
    comment: Option<String>,
    revision: i32,
    planned_at: Option<NaiveDateTime>,
    worn_at: Option<NaiveDateTime>,
    cancelled_at: Option<NaiveDateTime>,
    updated_at: NaiveDateTime,
}

const ROW_COLS: &str = "id, outfit_ref_id, status, wear_date, items_json, context_json, reaction, \
                        comment, revision, planned_at, worn_at, cancelled_at, updated_at";

/// 화면·MCP 에 내려 주는 기록 한 건.
#[derive(Debug, Clone, Serialize)]
pub struct WearRecord {
    pub record_id: String,
    pub outfit_ref: String,
    pub status: WearStatus,
    pub date: NaiveDate,
    pub items: Vec<ItemSnapshot>,
    pub occasion: Option<String>,
    pub mood_key: Option<String>,
    pub temperature: Option<f64>,
    pub reaction: Option<String>,
    pub comment: Option<String>,
    pub revision: i32,
    pub planned_at: Option<NaiveDateTime>,
    pub worn_at: Option<NaiveDateTime>,
    pub cancelled_at: Option<NaiveDateTime>,
    pub updated_at: NaiveDateTime,
}

impl From<WearRow> for WearRecord {
    fn from(r: WearRow) -> Self {
        let ctx = r.context_json.0;
        let text = |k: &str| ctx.get(k).and_then(Value::as_str).map(str::to_string);
        WearRecord {
            record_id: r.id,
            outfit_ref: r.outfit_ref_id,
            status: WearStatus::parse(&r.status),
            date: r.wear_date,
            items: r.items_json.0,
            occasion: text("occasion"),
            mood_key: text("mood_key"),
            temperature: ctx
                .get("weather")
                .and_then(|w| w.get("temperature"))
                .and_then(Value::as_f64),
            reaction: r.reaction,
            comment: r.comment,
            revision: r.revision,
            planned_at: r.planned_at,
            worn_at: r.worn_at,
            cancelled_at: r.cancelled_at,
            updated_at: r.updated_at,
        }
    }
}

/// 쓰기 결과. `outcome` 이 `already_*`·`unchanged`·`nothing_to_undo` 면 아무것도 바뀌지 않았다.
#[derive(Debug, Serialize)]
pub struct WriteResult {
    pub outcome: &'static str,
    pub record: WearRecord,
}

fn result(outcome: &'static str, row: WearRow) -> WriteResult {
    WriteResult {
        outcome,
        record: row.into(),
    }
}

fn is_retryable(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|c| c == "40001" || c == "1213" || c == "1205")
}

fn is_unique(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

async fn load(db: &MySqlPool, user_id: &str, id: &str) -> Result<WearRow, FlowError> {
    if id.is_empty() || id.len() > 36 {
        return Err(FlowError::RecordNotFound);
    }
    sqlx::query_as::<_, WearRow>(&format!(
        "SELECT {ROW_COLS} FROM outfit_wear WHERE id = ? AND user_id = ?"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(FlowError::RecordNotFound)
}

async fn find_active(
    db: &MySqlPool,
    user_id: &str,
    date: NaiveDate,
    key: &str,
) -> Result<Option<WearRow>, sqlx::Error> {
    sqlx::query_as::<_, WearRow>(&format!(
        "SELECT {ROW_COLS} FROM outfit_wear \
         WHERE user_id = ? AND wear_date = ? AND item_key_hash = SHA2(?, 256) AND active = 1"
    ))
    .bind(user_id)
    .bind(date)
    .bind(key)
    .fetch_optional(db)
    .await
}

/// 기록에 고정할 룩과 그때의 사진. 소유자·구성 검증은 라우트가 끝낸 값이다.
#[derive(Debug, Clone)]
pub struct Pin {
    pub look_id: String,
    pub image_path: Option<String>,
}

struct Insert<'a> {
    status: WearStatus,
    date: NaiveDate,
    reaction: Option<Reaction>,
    comment: Option<&'a str>,
    pin: Option<&'a Pin>,
}

/// 아직 룩이 없는 기록에 고정한다. 먼저 고정한 것이 남는다 — 재요청이 사진을 바꾸지 못한다.
async fn attach_pin(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    pin: Option<&Pin>,
) -> Result<(), sqlx::Error> {
    let Some(pin) = pin else { return Ok(()) };
    let has_photo = pin.image_path.is_some();
    sqlx::query(
        "UPDATE outfit_wear SET pinned_look_id = ?, pinned_image_path = ?, \
         photo_basis = IF(?, 'at_save', NULL), photo_pinned_at = IF(?, NOW(), NULL), \
         updated_at = updated_at \
         WHERE id = ? AND user_id = ? AND pinned_look_id IS NULL",
    )
    .bind(&pin.look_id)
    .bind(pin.image_path.as_deref())
    .bind(has_photo)
    .bind(has_photo)
    .bind(record_id)
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

async fn insert(
    db: &MySqlPool,
    user_id: &str,
    outfit: &OutfitRef,
    v: Insert<'_>,
) -> Result<String, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let planned = v.status == WearStatus::Planned;
    let has_photo = v.pin.is_some_and(|p| p.image_path.is_some());
    sqlx::query(
        "INSERT INTO outfit_wear \
         (id, user_id, outfit_ref_id, status, wear_date, item_key, items_json, context_json, \
          reaction, comment, planned_at, worn_at, \
          pinned_look_id, pinned_image_path, photo_basis, photo_pinned_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, IF(?, NOW(), NULL), IF(?, NULL, NOW()), \
          ?, ?, IF(?, 'at_save', NULL), IF(?, NOW(), NULL))",
    )
    .bind(&id)
    .bind(user_id)
    .bind(&outfit.id)
    .bind(v.status.as_str())
    .bind(v.date)
    .bind(&outfit.item_key)
    .bind(Json(&outfit.items))
    .bind(Json(&outfit.context))
    .bind(v.reaction.map(Reaction::as_str))
    .bind(v.comment)
    .bind(planned)
    .bind(planned)
    .bind(v.pin.map(|p| p.look_id.as_str()))
    .bind(v.pin.and_then(|p| p.image_path.as_deref()))
    .bind(has_photo)
    .bind(has_photo)
    .execute(db)
    .await?;
    Ok(id)
}

/// 입을 계획을 남긴다. 착용 기록은 만들지 않는다. 같은 날 같은 구성이 있으면 그대로 돌려준다.
pub async fn plan(
    db: &MySqlPool,
    user_id: &str,
    ref_id: &str,
    date: Option<NaiveDate>,
    comment: Option<&str>,
    pin: Option<&Pin>,
    today: NaiveDate,
) -> Result<WriteResult, FlowError> {
    let date = date.unwrap_or(today);
    check_plan_date(date, today)?;
    let comment = clean_comment(comment)?;
    let outfit = get_ref(db, user_id, ref_id).await?;
    for _ in 0..MAX_ATTEMPTS {
        if let Some(row) = find_active(db, user_id, date, &outfit.item_key).await? {
            attach_pin(db, user_id, &row.id, pin).await?;
            let outcome = match WearStatus::parse(&row.status) {
                WearStatus::Worn => "already_worn",
                _ => "already_planned",
            };
            return Ok(result(outcome, row));
        }
        ensure_items_owned(db, user_id, &outfit.items).await?;
        let v = Insert {
            status: WearStatus::Planned,
            date,
            reaction: None,
            comment: comment.as_deref(),
            pin,
        };
        match insert(db, user_id, &outfit, v).await {
            Ok(id) => return Ok(result("planned", load(db, user_id, &id).await?)),
            Err(e) if is_unique(&e) || is_retryable(&e) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FlowError::Busy)
}

pub struct WornInput<'a> {
    pub date: Option<NaiveDate>,
    pub reaction: Option<Reaction>,
    pub comment: Option<&'a str>,
}

/// 코디 참조로 "입었다" 를 남긴다. 같은 날 같은 구성의 계획이 있으면 그 행을 확정하고, 없으면 새로
/// 만든다. 이미 입은 기록이 있으면 그대로 돌려준다(반응·메모도 바꾸지 않는다).
pub async fn worn_by_ref(
    db: &MySqlPool,
    user_id: &str,
    ref_id: &str,
    input: WornInput<'_>,
    pin: Option<&Pin>,
    today: NaiveDate,
) -> Result<WriteResult, FlowError> {
    let date = input.date.unwrap_or(today);
    check_worn_date(date, today)?;
    let comment = clean_comment(input.comment)?;
    let outfit = get_ref(db, user_id, ref_id).await?;
    for _ in 0..MAX_ATTEMPTS {
        match find_active(db, user_id, date, &outfit.item_key).await? {
            Some(row) if WearStatus::parse(&row.status) == WearStatus::Worn => {
                attach_pin(db, user_id, &row.id, pin).await?;
                return Ok(result("already_worn", row));
            }
            Some(row) => {
                attach_pin(db, user_id, &row.id, pin).await?;
                let done = sqlx::query(
                    "UPDATE outfit_wear SET status = 'worn', worn_at = NOW(), reaction = ?, \
                     comment = COALESCE(?, comment), revision = revision + 1 \
                     WHERE id = ? AND user_id = ? AND status = 'planned'",
                )
                .bind(input.reaction.map(Reaction::as_str))
                .bind(comment.as_deref())
                .bind(&row.id)
                .bind(user_id)
                .execute(db)
                .await;
                match done {
                    Ok(r) if r.rows_affected() == 1 => {
                        return Ok(result("worn", load(db, user_id, &row.id).await?));
                    }
                    Ok(_) => continue,
                    Err(e) if is_retryable(&e) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            None => {
                ensure_items_owned(db, user_id, &outfit.items).await?;
                let v = Insert {
                    status: WearStatus::Worn,
                    date,
                    reaction: input.reaction,
                    comment: comment.as_deref(),
                    pin,
                };
                match insert(db, user_id, &outfit, v).await {
                    Ok(id) => return Ok(result("worn", load(db, user_id, &id).await?)),
                    Err(e) if is_unique(&e) || is_retryable(&e) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    Err(FlowError::Busy)
}

/// 이미 있는 계획을 "입었다" 로 확정한다. 재시도는 `already_worn` 으로 끝난다.
pub async fn confirm_record(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    expected_revision: i32,
    input: WornInput<'_>,
    today: NaiveDate,
) -> Result<WriteResult, FlowError> {
    let comment = clean_comment(input.comment)?;
    for _ in 0..MAX_ATTEMPTS {
        let row = load(db, user_id, record_id).await?;
        match WearStatus::parse(&row.status) {
            WearStatus::Worn => return Ok(result("already_worn", row)),
            WearStatus::Cancelled => {
                return Err(FlowError::InvalidTransition(WearStatus::Cancelled));
            }
            WearStatus::Planned => {}
        }
        if row.revision != expected_revision {
            return Err(FlowError::RevisionConflict(row.revision));
        }
        let date = input.date.unwrap_or(row.wear_date);
        check_worn_date(date, today)?;
        let done = sqlx::query(
            "UPDATE outfit_wear SET status = 'worn', worn_at = NOW(), wear_date = ?, reaction = ?, \
             comment = COALESCE(?, comment), revision = revision + 1 \
             WHERE id = ? AND user_id = ? AND status = 'planned' AND revision = ?",
        )
        .bind(date)
        .bind(input.reaction.map(Reaction::as_str))
        .bind(comment.as_deref())
        .bind(record_id)
        .bind(user_id)
        .bind(expected_revision)
        .execute(db)
        .await;
        match done {
            Ok(r) if r.rows_affected() == 1 => {
                return Ok(result("worn", load(db, user_id, record_id).await?));
            }
            Ok(_) => continue,
            Err(e) if is_unique(&e) => return Err(FlowError::DuplicateOnDate),
            Err(e) if is_retryable(&e) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FlowError::Busy)
}

/// 계획을 취소한다. 입은 기록은 되돌리기로만 바꾼다.
pub async fn cancel_record(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    expected_revision: i32,
) -> Result<WriteResult, FlowError> {
    for _ in 0..MAX_ATTEMPTS {
        let row = load(db, user_id, record_id).await?;
        match WearStatus::parse(&row.status) {
            WearStatus::Cancelled => return Ok(result("already_cancelled", row)),
            WearStatus::Worn => return Err(FlowError::InvalidTransition(WearStatus::Worn)),
            WearStatus::Planned => {}
        }
        if row.revision != expected_revision {
            return Err(FlowError::RevisionConflict(row.revision));
        }
        let done = sqlx::query(
            "UPDATE outfit_wear SET status = 'cancelled', cancelled_at = NOW(), \
             revision = revision + 1 \
             WHERE id = ? AND user_id = ? AND status = 'planned' AND revision = ?",
        )
        .bind(record_id)
        .bind(user_id)
        .bind(expected_revision)
        .execute(db)
        .await;
        match done {
            Ok(r) if r.rows_affected() == 1 => {
                return Ok(result("cancelled", load(db, user_id, record_id).await?));
            }
            Ok(_) => continue,
            Err(e) if is_retryable(&e) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FlowError::Busy)
}

/// 잘못 남긴 착용을 되돌린다. 계획에서 온 기록은 계획으로, 바로 남긴 기록은 취소로 간다.
/// 이미 입은 기록이 아니면 `nothing_to_undo` 로 끝난다(재시도).
pub async fn undo_record(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    expected_revision: i32,
) -> Result<WriteResult, FlowError> {
    for _ in 0..MAX_ATTEMPTS {
        let row = load(db, user_id, record_id).await?;
        if WearStatus::parse(&row.status) != WearStatus::Worn {
            return Ok(result("nothing_to_undo", row));
        }
        if row.revision != expected_revision {
            return Err(FlowError::RevisionConflict(row.revision));
        }
        let to_plan = row.planned_at.is_some();
        let done = sqlx::query(
            "UPDATE outfit_wear SET status = ?, worn_at = NULL, reaction = NULL, \
             cancelled_at = IF(?, NULL, NOW()), revision = revision + 1 \
             WHERE id = ? AND user_id = ? AND status = 'worn' AND revision = ?",
        )
        .bind(if to_plan { "planned" } else { "cancelled" })
        .bind(to_plan)
        .bind(record_id)
        .bind(user_id)
        .bind(expected_revision)
        .execute(db)
        .await;
        match done {
            Ok(r) if r.rows_affected() == 1 => {
                let outcome = if to_plan {
                    "undone_to_planned"
                } else {
                    "undone_to_cancelled"
                };
                return Ok(result(outcome, load(db, user_id, record_id).await?));
            }
            Ok(_) => continue,
            Err(e) if is_retryable(&e) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FlowError::Busy)
}

/// 고칠 내용. 바깥 `None` 은 그대로 두라는 뜻, 안쪽 `None` 은 지우라는 뜻이다.
#[derive(Default)]
pub struct Edit<'a> {
    pub date: Option<NaiveDate>,
    pub reaction: Option<Option<Reaction>>,
    pub comment: Option<Option<&'a str>>,
}

/// 날짜·반응·메모를 고친다. 이미 같은 값이면 `unchanged` 로 끝나 재시도가 충돌하지 않는다.
pub async fn edit_record(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    expected_revision: i32,
    edit: Edit<'_>,
    today: NaiveDate,
) -> Result<WriteResult, FlowError> {
    let comment = match edit.comment {
        Some(c) => Some(clean_comment(c)?),
        None => None,
    };
    for _ in 0..MAX_ATTEMPTS {
        let row = load(db, user_id, record_id).await?;
        let status = WearStatus::parse(&row.status);
        if status == WearStatus::Cancelled {
            return Err(FlowError::InvalidTransition(WearStatus::Cancelled));
        }
        let date = edit.date.unwrap_or(row.wear_date);
        let reaction = match edit.reaction {
            Some(r) => r.map(|r| r.as_str().to_string()),
            None => row.reaction.clone(),
        };
        let next_comment = comment.clone().unwrap_or_else(|| row.comment.clone());
        if reaction.is_some() && status != WearStatus::Worn {
            return Err(FlowError::ReactionNeedsWorn);
        }
        if date == row.wear_date && reaction == row.reaction && next_comment == row.comment {
            return Ok(result("unchanged", row));
        }
        if row.revision != expected_revision {
            return Err(FlowError::RevisionConflict(row.revision));
        }
        if date != row.wear_date {
            match status {
                WearStatus::Worn => check_worn_date(date, today)?,
                _ => check_plan_date(date, today)?,
            }
        }
        let done = sqlx::query(
            "UPDATE outfit_wear SET wear_date = ?, reaction = ?, comment = ?, \
             revision = revision + 1 \
             WHERE id = ? AND user_id = ? AND revision = ? AND status = ?",
        )
        .bind(date)
        .bind(&reaction)
        .bind(&next_comment)
        .bind(record_id)
        .bind(user_id)
        .bind(expected_revision)
        .bind(status.as_str())
        .execute(db)
        .await;
        match done {
            Ok(r) if r.rows_affected() == 1 => {
                return Ok(result("updated", load(db, user_id, record_id).await?));
            }
            Ok(_) => continue,
            Err(e) if is_unique(&e) => return Err(FlowError::DuplicateOnDate),
            Err(e) if is_retryable(&e) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FlowError::Busy)
}

/// 기록 한 건과 저장 때 고정한 룩·사진.
#[derive(Debug)]
pub struct RecordOrigin {
    pub record: WearRecord,
    pub source: String,
    /// 룩북에서 만든 참조의 룩 id. 고정 전에 그 룩이 지워졌으면 이 값만 남는다.
    pub ref_look_id: Option<String>,
    pub pinned_look_id: Option<String>,
    pub pinned_image_path: Option<String>,
    pub photo_basis: Option<String>,
}

/// 내 기록 한 건. 상태와 상관없이 읽는다. 다른 계정의 것이면 없는 것과 같다.
pub async fn record_origin(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
) -> Result<RecordOrigin, FlowError> {
    let row = load(db, user_id, record_id).await?;
    let (source, dedupe_key): (String, String) =
        sqlx::query_as("SELECT source, dedupe_key FROM outfit_ref WHERE id = ? AND user_id = ?")
            .bind(&row.outfit_ref_id)
            .bind(user_id)
            .fetch_one(db)
            .await?;
    let (pinned_look_id, pinned_image_path, photo_basis): (
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT pinned_look_id, pinned_image_path, photo_basis FROM outfit_wear \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&row.id)
    .bind(user_id)
    .fetch_one(db)
    .await?;
    Ok(RecordOrigin {
        record: row.into(),
        ref_look_id: look_of(&source, &dedupe_key),
        source,
        pinned_look_id,
        pinned_image_path,
        photo_basis,
    })
}

/// 내 룩 한 장. 사진 data URI 가 든 `outfit_json` 은 읽지 않고 옷 id 만 꺼낸다.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OwnedLook {
    pub id: String,
    pub title: String,
    pub mood_key: Option<String>,
    pub image_path: Option<String>,
    pub image_prompt: String,
    pub item_ids: Option<Json<Vec<String>>>,
    pub item_count: i64,
}

impl OwnedLook {
    /// 이 룩의 옷 id 구성이 코디와 정확히 같은가. 옷 id 가 빠진 예전 룩은 같다고 하지 않는다.
    pub fn same_items(&self, key: &str) -> bool {
        self.item_ids.as_ref().is_some_and(|ids| {
            ids.0.len() as i64 == self.item_count
                && item_key(ids.0.iter().map(String::as_str)) == key
        })
    }
}

/// 내 룩. 다른 계정의 것이면 없는 것과 같다.
pub async fn get_look(
    db: &MySqlPool,
    user_id: &str,
    look_id: &str,
) -> Result<Option<OwnedLook>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, title, mood_key, image_path, image_prompt, \
         JSON_EXTRACT(outfit_json, '$[*].id') AS item_ids, \
         CAST(JSON_LENGTH(outfit_json) AS SIGNED) AS item_count \
         FROM lookbook_look WHERE id = ? AND user_id = ?",
    )
    .bind(look_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// 이 경로가 내 사진인가(`routes::images` 와 같은 기준). 그렇다면 사진 종류를 돌려준다.
pub async fn owned_image(
    db: &MySqlPool,
    user_id: &str,
    path: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT CAST(presentation AS CHAR) FROM outfit_image \
         WHERE image_path = ? AND user_id = ? LIMIT 1",
    )
    .bind(path)
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// 고정된 룩의 첫 사진을 이어 받는다. 이미 사진이 있으면 바꾸지 않는다. revision·updated_at 은 그대로다 —
/// 사용자가 바꾼 내용이 아니다.
pub async fn adopt_look_photo(
    db: &MySqlPool,
    user_id: &str,
    record_id: &str,
    look_id: &str,
    path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE outfit_wear SET pinned_image_path = ?, photo_basis = 'later', \
         photo_pinned_at = NOW(), updated_at = updated_at \
         WHERE id = ? AND user_id = ? AND pinned_look_id = ? AND pinned_image_path IS NULL",
    )
    .bind(path)
    .bind(record_id)
    .bind(user_id)
    .bind(look_id)
    .execute(db)
    .await?;
    Ok(())
}

pub struct HistoryQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub status: Option<WearStatus>,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct HistoryPage {
    pub today: NaiveDate,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub records: Vec<WearRecord>,
    /// 범위 안에 더 있다. 날짜를 좁혀 다시 읽는다.
    pub has_more: bool,
}

/// 내 기록. 최근 날짜부터.
pub async fn history(
    db: &MySqlPool,
    user_id: &str,
    q: HistoryQuery,
    today: NaiveDate,
) -> Result<HistoryPage, FlowError> {
    if q.from > q.to || (q.to - q.from).num_days() > HISTORY_MAX_RANGE_DAYS {
        return Err(FlowError::InvalidRange);
    }
    let limit = q.limit.clamp(1, HISTORY_MAX_LIMIT);
    let mut sql = format!(
        "SELECT {ROW_COLS} FROM outfit_wear WHERE user_id = ? AND wear_date BETWEEN ? AND ?"
    );
    if q.status.is_some() {
        sql.push_str(" AND status = ?");
    }
    sql.push_str(" ORDER BY wear_date DESC, created_at DESC, id LIMIT ?");
    let mut query = sqlx::query_as::<_, WearRow>(&sql)
        .bind(user_id)
        .bind(q.from)
        .bind(q.to);
    if let Some(s) = q.status {
        query = query.bind(s.as_str());
    }
    let mut rows = query.bind(limit + 1).fetch_all(db).await?;
    let has_more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    Ok(HistoryPage {
        today,
        from: q.from,
        to: q.to,
        records: rows.into_iter().map(WearRecord::from).collect(),
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn today_uses_seoul_not_server_clock() {
        // 서울은 UTC+9 다. 값이 UTC 날짜와 같거나 하루 뒤여야 한다.
        let diff = (today_kst() - Utc::now().date_naive()).num_days();
        assert!(diff == 0 || diff == 1, "{diff}");
    }

    #[test]
    fn plan_dates_are_today_or_later_within_limit() {
        let today = d("2026-11-04");
        assert!(check_plan_date(today, today).is_ok());
        assert!(check_plan_date(today + Duration::days(60), today).is_ok());
        assert_eq!(
            check_plan_date(today - Duration::days(1), today)
                .unwrap_err()
                .code(),
            "date_in_past"
        );
        assert_eq!(
            check_plan_date(today + Duration::days(61), today)
                .unwrap_err()
                .code(),
            "date_too_far"
        );
    }

    #[test]
    fn worn_dates_are_past_only() {
        let today = d("2026-11-04");
        assert!(check_worn_date(today, today).is_ok());
        assert!(check_worn_date(today - Duration::days(365), today).is_ok());
        assert_eq!(
            check_worn_date(today + Duration::days(1), today)
                .unwrap_err()
                .code(),
            "date_in_future"
        );
        assert_eq!(
            check_worn_date(today - Duration::days(366), today)
                .unwrap_err()
                .code(),
            "date_too_old"
        );
    }

    #[test]
    fn comment_is_trimmed_and_bounded() {
        assert_eq!(
            clean_comment(Some("  좋아요\u{0007} ")).unwrap().as_deref(),
            Some("좋아요")
        );
        assert_eq!(clean_comment(Some("   ")).unwrap(), None);
        assert_eq!(clean_comment(None).unwrap(), None);
        assert!(clean_comment(Some(&"가".repeat(200))).is_ok());
        assert_eq!(
            clean_comment(Some(&"가".repeat(201))).unwrap_err().code(),
            "comment_too_long"
        );
    }
}
