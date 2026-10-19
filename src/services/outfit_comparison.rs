//! 홈 추천의 두 코디를 나란히 두고 고르는 개인 취향 비교(마이그레이션 `20261020000001`).
//!
//! 흐름:
//!   1. 홈 추천(`/recommendation/multi`)이 응답을 만들 때 [`record_batch`] 로 그 응답의 코디를
//!      서버에 남긴다. 추가 모델 호출은 없다.
//!   2. 사용자가 비교를 열면 [`create_comparison`] 이 그 묶음에서 서로 다른 두 코디를 꺼내 좌우를
//!      한 번 정하고 노출로 남긴다. 화면이 보낸 코디·점수·이름은 받지 않는다 — 묶음 id 하나만 받는다.
//!   3. 고르면 [`submit_vote`] 가 응답을 남긴다. 비교 하나에 한 번, 먼저 쓴 것이 남는다.
//!
//! 기록만 한다. 추천 점수·순위에는 쓰지 않는다.

use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::models::clothing::Clothing;
use crate::models::recommendation::ModeRecommendation;

/// 비교를 어떤 모습으로 보여 줬는지. 옷 이름·카테고리·색 글자와 색 점으로 보여 주고, 옷장에
/// 같은 이름의 옷 사진이 남아 있으면 작은 사진을 곁들인다. 생성한 모델 착용 이미지로 비교한
/// 데이터가 아니다 — 나중에 해석할 때 이 값으로 가른다. 표현 방식을 바꾸면 값도 바꾼다.
pub const REPRESENTATION: &str = "garment_text_and_color_with_optional_current_thumbnails";

/// 한 줄 메모의 최대 길이(글자 수). 표의 `comment VARCHAR(200)` 과 같다.
pub const COMMENT_MAX_CHARS: usize = 200;

/// 보여 준 아이템 한 벌. 옷이 나중에 바뀌거나 지워져도 그때의 값을 남긴다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSnapshot {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: Option<String>,
    pub sub_category: Option<String>,
    pub material: Option<String>,
    /// 그때 옷장에 사진이 있었는지. 사진 자체(수 MB 의 data URI)는 복사하지 않는다.
    pub has_image: bool,
}

/// 추천 응답에 실린 코디 하나.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateSnapshot {
    pub mode: String,
    /// 응답 안의 순서(0부터).
    pub position: u8,
    pub item_key: String,
    pub items: Vec<ItemSnapshot>,
    pub final_score: i32,
    pub style_score: i32,
}

/// 같은 코디인지 가르는 키. 순서와 상관없이 정렬한 아이템 id 를 잇는다.
pub fn item_key<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
    let mut ids: Vec<&str> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids.join("|")
}

/// 응답의 코디들을 값으로 복사한다. 색·세부 카테고리는 응답에 없어 옷장 행에서 읽는다.
pub fn snapshot_modes(
    modes: &[ModeRecommendation],
    clothes: &[Clothing],
) -> Vec<CandidateSnapshot> {
    modes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let items: Vec<ItemSnapshot> = m
                .outfit
                .iter()
                .map(|o| {
                    let c = clothes.iter().find(|c| c.id == o.id);
                    ItemSnapshot {
                        id: o.id.clone(),
                        name: o.name.clone(),
                        category: o.category.clone(),
                        color: c.and_then(|c| c.color.clone()),
                        sub_category: c.and_then(|c| c.sub_category.clone()),
                        material: o.material.clone(),
                        has_image: o.image_url.as_deref().is_some_and(|u| !u.is_empty()),
                    }
                })
                .collect();
            CandidateSnapshot {
                mode: m.mode.clone(),
                position: u8::try_from(i).unwrap_or(u8::MAX),
                item_key: item_key(items.iter().map(|i| i.id.as_str())),
                items,
                final_score: m.scoring_detail.final_score,
                style_score: m.scoring_detail.style_score,
            }
        })
        .collect()
}

/// 비교할 두 코디. 응답 순서대로 보며 앞의 것과 아이템 구성이 다른 첫 코디를 짝으로 고른다.
/// 서로 다른 코디가 둘이 안 되면 `None`.
pub fn pick_pair(candidates: &[CandidateSnapshot]) -> Option<(usize, usize)> {
    let first = candidates.iter().position(|c| !c.item_key.is_empty())?;
    let second = candidates
        .iter()
        .enumerate()
        .skip(first + 1)
        .find(|(_, c)| !c.item_key.is_empty() && c.item_key != candidates[first].item_key)?
        .0;
    Some((first, second))
}

// ─── 응답 ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    A,
    B,
    Neither,
}

impl Choice {
    pub fn as_str(self) -> &'static str {
        match self {
            Choice::A => "a",
            Choice::B => "b",
            Choice::Neither => "neither",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "a" => Some(Choice::A),
            "b" => Some(Choice::B),
            "neither" => Some(Choice::Neither),
            _ => None,
        }
    }

    /// 이유가 무엇을 말하는지. A·B 는 고른 쪽의 좋은 점, 둘 다 아쉬움은 두 코디의 아쉬운 점이다.
    pub fn reason_polarity(self) -> &'static str {
        match self {
            Choice::A | Choice::B => "chosen",
            Choice::Neither => "both_lacking",
        }
    }
}

/// 고른 이유(또는 둘 다 아쉬운 이유). 방향은 [`Choice::reason_polarity`] 가 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// 색 조합
    ColorCombo,
    /// 실루엣
    Silhouette,
    /// 내 취향
    PersonalTaste,
    /// 실용성/착용 편의
    Practicality,
    /// 날씨 적합성
    WeatherFit,
}

/// 화면이 보내는 응답. 사용자 id·코디·점수는 받지 않는다 — 모르는 필드가 오면 거절한다.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoteRequest {
    pub choice: Choice,
    #[serde(default)]
    pub reasons: Vec<ReasonCode>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// 검증을 마친 응답. 이유는 정렬·중복 제거, 메모는 앞뒤 공백을 뗀다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    pub choice: Choice,
    pub reasons: Vec<ReasonCode>,
    pub comment: Option<String>,
}

/// 검증 실패. 라우트가 400 으로 돌려준다.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidVote(pub String);

pub fn validate_vote(req: VoteRequest) -> Result<Vote, InvalidVote> {
    let mut reasons = req.reasons;
    reasons.sort_unstable();
    reasons.dedup();

    let comment = match req.comment {
        None => None,
        Some(raw) => {
            // 줄바꿈 말고 제어 문자는 버린다 — 저장해 두고 다시 화면에 그릴 값이다.
            let cleaned: String = raw
                .chars()
                .filter(|c| *c == '\n' || !c.is_control())
                .collect();
            let trimmed = cleaned.trim();
            if trimmed.chars().count() > COMMENT_MAX_CHARS {
                return Err(InvalidVote(format!(
                    "메모는 {COMMENT_MAX_CHARS}자 이내로 적어 주세요."
                )));
            }
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
    };

    Ok(Vote {
        choice: req.choice,
        reasons,
        comment,
    })
}

// ─── 저장 ───

/// 추천 응답 한 번의 맥락. 무드·날씨는 서버가 실제로 쓴 값이다.
pub struct BatchContext<'a> {
    pub gender: Option<&'a str>,
    pub mood_key: Option<&'a str>,
    pub occasion: Option<&'a str>,
    pub weather: serde_json::Value,
    pub provenance: serde_json::Value,
}

/// 추천 응답을 남기고 묶음 id 를 돌려준다. 코디가 하나도 없으면 남기지 않는다.
pub async fn record_batch(
    db: &MySqlPool,
    user_id: &str,
    ctx: &BatchContext<'_>,
    candidates: &[CandidateSnapshot],
) -> Result<Option<String>, sqlx::Error> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO recommendation_batch \
         (id, user_id, gender, mood_key, occasion, weather_json, candidates_json, provenance_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(ctx.gender)
    .bind(ctx.mood_key)
    .bind(ctx.occasion)
    .bind(&ctx.weather)
    .bind(sqlx::types::Json(candidates))
    .bind(&ctx.provenance)
    .execute(db)
    .await?;
    Ok(Some(id))
}

/// 화면에 보여 줄 한쪽 코디.
#[derive(Debug, Clone, Serialize)]
pub struct ComparisonOption {
    /// "a" 또는 "b". 화면 자리다.
    pub slot: &'static str,
    pub items: Vec<ItemSnapshot>,
}

/// 비교 맥락 중 화면에 보여도 되는 것만.
#[derive(Debug, Clone, Serialize)]
pub struct ComparisonContextView {
    pub mood_key: Option<String>,
    pub temperature: Option<f64>,
    pub weather_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoteView {
    pub choice: Choice,
    pub reasons: Vec<ReasonCode>,
    pub comment: Option<String>,
    pub submitted_at: chrono::NaiveDateTime,
}

/// 화면에 내려 주는 비교. 모드 이름·점수·버전 정보는 넣지 않는다 — 고르는 데 영향을 주거나
/// 쓸모없는 기술 정보다.
#[derive(Debug, Clone, Serialize)]
pub struct ComparisonView {
    pub id: String,
    pub created_at: chrono::NaiveDateTime,
    pub context: ComparisonContextView,
    pub options: [ComparisonOption; 2],
    pub vote: Option<VoteView>,
}

#[derive(Debug)]
pub enum CreateError {
    /// 이 사용자의 추천 묶음이 아니거나 없다.
    BatchNotFound,
    /// 서로 다른 코디가 둘이 안 된다.
    NotEnoughCandidates,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for CreateError {
    fn from(e: sqlx::Error) -> Self {
        CreateError::Db(e)
    }
}

/// 묶음에서 비교를 만든다. 이미 있으면 그것을 그대로 돌려준다(좌우도 처음 정한 대로).
///
/// `a_first` 는 응답 순서상 앞선 코디를 A 자리에 둘지다. 라우트가 무작위로 정해 넘긴다 —
/// 처음 만들 때만 쓰이고 저장된 뒤에는 바뀌지 않는다.
pub async fn create_comparison(
    db: &MySqlPool,
    user_id: &str,
    batch_id: &str,
    a_first: bool,
) -> Result<ComparisonView, CreateError> {
    if let Some(view) = find_by_batch(db, user_id, batch_id).await? {
        return Ok(view);
    }

    let batch: Option<BatchRow> = sqlx::query_as(
        "SELECT candidates_json, gender, mood_key, occasion, weather_json, provenance_json, \
         created_at FROM recommendation_batch WHERE id = ? AND user_id = ?",
    )
    .bind(batch_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((candidates, gender, mood_key, occasion, weather, provenance, batch_at)) = batch
    else {
        return Err(CreateError::BatchNotFound);
    };
    let candidates = candidates.0;
    let Some((hi, lo)) = pick_pair(&candidates) else {
        return Err(CreateError::NotEnoughCandidates);
    };
    let (a, b) = if a_first {
        (&candidates[hi], &candidates[lo])
    } else {
        (&candidates[lo], &candidates[hi])
    };

    let context = serde_json::json!({
        "batch_created_at": batch_at,
        "gender": gender,
        "mood_key": mood_key,
        "occasion": occasion,
        "weather": weather,
        "provenance": provenance,
        "representation": REPRESENTATION,
    });

    // 같은 묶음으로 동시에 두 번 들어와도 한 행만 남는다. `INSERT IGNORE` 는 CHECK 위반까지
    // 경고로 삼키므로 쓰지 않는다 — 중복 키만 넘긴다.
    sqlx::query(
        "INSERT INTO outfit_comparison \
         (id, user_id, batch_id, a_mode, a_position, a_item_key, a_items_json, \
          b_mode, b_position, b_item_key, b_items_json, a_is_higher_ranked, context_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE id = id",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(batch_id)
    .bind(&a.mode)
    .bind(a.position)
    .bind(&a.item_key)
    .bind(sqlx::types::Json(&a.items))
    .bind(&b.mode)
    .bind(b.position)
    .bind(&b.item_key)
    .bind(sqlx::types::Json(&b.items))
    .bind(a_first)
    .bind(&context)
    .execute(db)
    .await?;

    find_by_batch(db, user_id, batch_id)
        .await?
        .ok_or(CreateError::Db(sqlx::Error::RowNotFound))
}

/// `recommendation_batch` 한 행: 코디·성별·무드·상황·날씨·출처·시각.
type BatchRow = (
    sqlx::types::Json<Vec<CandidateSnapshot>>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<serde_json::Value>,
    serde_json::Value,
    chrono::NaiveDateTime,
);

/// `outfit_comparison_vote` 한 행: 선택·이유·메모·시각.
type VoteRow = (
    String,
    sqlx::types::Json<Vec<ReasonCode>>,
    Option<String>,
    chrono::NaiveDateTime,
);

type ComparisonRow = (
    String,
    sqlx::types::Json<Vec<ItemSnapshot>>,
    sqlx::types::Json<Vec<ItemSnapshot>>,
    serde_json::Value,
    chrono::NaiveDateTime,
);

const COMPARISON_COLUMNS: &str = "id, a_items_json, b_items_json, context_json, created_at";

async fn find_by_batch(
    db: &MySqlPool,
    user_id: &str,
    batch_id: &str,
) -> Result<Option<ComparisonView>, sqlx::Error> {
    let row: Option<ComparisonRow> = sqlx::query_as(&format!(
        "SELECT {COMPARISON_COLUMNS} FROM outfit_comparison WHERE user_id = ? AND batch_id = ?"
    ))
    .bind(user_id)
    .bind(batch_id)
    .fetch_optional(db)
    .await?;
    match row {
        Some(r) => Ok(Some(to_view(db, user_id, r).await?)),
        None => Ok(None),
    }
}

/// 내 비교 하나. 다른 사용자의 비교는 없는 것과 같다.
pub async fn get_comparison(
    db: &MySqlPool,
    user_id: &str,
    id: &str,
) -> Result<Option<ComparisonView>, sqlx::Error> {
    let row: Option<ComparisonRow> = sqlx::query_as(&format!(
        "SELECT {COMPARISON_COLUMNS} FROM outfit_comparison WHERE id = ? AND user_id = ?"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    match row {
        Some(r) => Ok(Some(to_view(db, user_id, r).await?)),
        None => Ok(None),
    }
}

async fn to_view(
    db: &MySqlPool,
    user_id: &str,
    (id, a, b, context, created_at): ComparisonRow,
) -> Result<ComparisonView, sqlx::Error> {
    let vote = find_vote(db, user_id, &id).await?;
    let weather = context.get("weather");
    Ok(ComparisonView {
        context: ComparisonContextView {
            mood_key: context
                .get("mood_key")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            temperature: weather
                .and_then(|w| w.get("temperature"))
                .and_then(|v| v.as_f64()),
            weather_description: weather
                .and_then(|w| w.get("weather_description"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        id,
        created_at,
        options: [
            ComparisonOption {
                slot: "a",
                items: a.0,
            },
            ComparisonOption {
                slot: "b",
                items: b.0,
            },
        ],
        vote,
    })
}

async fn find_vote(
    db: &MySqlPool,
    user_id: &str,
    comparison_id: &str,
) -> Result<Option<VoteView>, sqlx::Error> {
    let row: Option<VoteRow> = sqlx::query_as(
        "SELECT choice, reasons_json, comment, submitted_at FROM outfit_comparison_vote \
         WHERE comparison_id = ? AND user_id = ?",
    )
    .bind(comparison_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(choice, reasons, comment, submitted_at)| {
        Some(VoteView {
            choice: Choice::parse(&choice)?,
            reasons: reasons.0,
            comment,
            submitted_at,
        })
    }))
}

#[derive(Debug)]
pub enum VoteOutcome {
    /// 이번에 저장했다.
    Saved(VoteView),
    /// 같은 응답이 이미 있었다(다시 보내기·새로고침). 저장된 것을 그대로 돌려준다.
    AlreadySaved(VoteView),
    /// 다른 응답이 이미 있다. 먼저 쓴 것이 남는다.
    Conflict,
}

#[derive(Debug)]
pub enum VoteError {
    /// 내 비교가 아니거나 없다.
    NotFound,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for VoteError {
    fn from(e: sqlx::Error) -> Self {
        VoteError::Db(e)
    }
}

/// 응답을 남긴다. 비교 하나에 한 번이고, 먼저 쓴 것이 남는다(동시 제출도 키가 막는다).
pub async fn submit_vote(
    db: &MySqlPool,
    user_id: &str,
    comparison_id: &str,
    vote: &Vote,
) -> Result<VoteOutcome, VoteError> {
    // 고른 코디의 정체는 서버에 남은 노출에서 읽는다. 화면이 보낸 이름·id 는 받지 않는다.
    let exposure: Option<(String, i8, String, String, i8, String)> = sqlx::query_as(
        "SELECT a_mode, a_position, a_item_key, b_mode, b_position, b_item_key \
         FROM outfit_comparison WHERE id = ? AND user_id = ?",
    )
    .bind(comparison_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((a_mode, a_pos, a_key, b_mode, b_pos, b_key)) = exposure else {
        return Err(VoteError::NotFound);
    };
    let chosen = match vote.choice {
        Choice::A => Some((a_mode, a_pos, a_key)),
        Choice::B => Some((b_mode, b_pos, b_key)),
        Choice::Neither => None,
    };
    let (chosen_mode, chosen_pos, chosen_key) = match chosen {
        Some((m, p, k)) => (Some(m), Some(p), Some(k)),
        None => (None, None, None),
    };

    // 그냥 넣고, 중복 키면 이미 있는 응답과 견준다. `ON DUPLICATE KEY` 로는 새로 넣었는지
    // 알 수 없다 — sqlx 가 CLIENT_FOUND_ROWS 로 연결해 중복도 1행으로 센다.
    let inserted = sqlx::query(
        "INSERT INTO outfit_comparison_vote \
         (comparison_id, user_id, choice, chosen_mode, chosen_position, chosen_item_key, \
          reason_polarity, reasons_json, comment) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(comparison_id)
    .bind(user_id)
    .bind(vote.choice.as_str())
    .bind(chosen_mode)
    .bind(chosen_pos)
    .bind(chosen_key)
    .bind(vote.choice.reason_polarity())
    .bind(sqlx::types::Json(&vote.reasons))
    .bind(&vote.comment)
    .execute(db)
    .await;
    let fresh = match inserted {
        Ok(_) => true,
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            false
        }
        Err(e) => return Err(e.into()),
    };

    let saved = find_vote(db, user_id, comparison_id)
        .await?
        .ok_or(VoteError::Db(sqlx::Error::RowNotFound))?;
    if fresh {
        return Ok(VoteOutcome::Saved(saved));
    }
    let same = saved.choice == vote.choice
        && saved.reasons == vote.reasons
        && saved.comment == vote.comment;
    Ok(if same {
        VoteOutcome::AlreadySaved(saved)
    } else {
        VoteOutcome::Conflict
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(mode: &str, position: u8, ids: &[&str]) -> CandidateSnapshot {
        CandidateSnapshot {
            mode: mode.to_string(),
            position,
            item_key: item_key(ids.iter().copied()),
            items: ids
                .iter()
                .map(|id| ItemSnapshot {
                    id: id.to_string(),
                    name: format!("옷 {id}"),
                    category: "상의".to_string(),
                    color: None,
                    sub_category: None,
                    material: None,
                    has_image: false,
                })
                .collect(),
            final_score: 0,
            style_score: 0,
        }
    }

    #[test]
    fn item_key_ignores_order_and_duplicates() {
        assert_eq!(item_key(["b", "a", "c"]), item_key(["c", "b", "a"]));
        assert_eq!(item_key(["a", "a", "b"]), "a|b");
    }

    #[test]
    fn pair_skips_the_same_outfit_in_another_order() {
        let c = [
            cand("todays_pick", 0, &["t1", "b1"]),
            cand("variation", 1, &["b1", "t1"]),
            cand("dormant_revival", 2, &["t2", "b1"]),
        ];
        assert_eq!(pick_pair(&c), Some((0, 2)));
    }

    #[test]
    fn fewer_than_two_distinct_outfits_has_no_pair() {
        assert_eq!(pick_pair(&[]), None);
        assert_eq!(pick_pair(&[cand("todays_pick", 0, &["t1"])]), None);
        let same = [
            cand("todays_pick", 0, &["t1", "b1"]),
            cand("variation", 1, &["b1", "t1"]),
        ];
        assert_eq!(pick_pair(&same), None);
        // 아이템이 하나도 없는 코디는 비교할 수 없다.
        let empty = [cand("todays_pick", 0, &[]), cand("variation", 1, &["t1"])];
        assert_eq!(pick_pair(&empty), None);
    }

    fn req(json: &str) -> Result<VoteRequest, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn votes_parse_and_normalise() {
        let v = validate_vote(
            req(r#"{"choice":"neither","reasons":["weather_fit","color_combo","weather_fit"],"comment":"  둘 다 무거워요 \u0007 "}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(v.choice, Choice::Neither);
        assert_eq!(v.reasons, [ReasonCode::ColorCombo, ReasonCode::WeatherFit]);
        assert_eq!(v.comment.as_deref(), Some("둘 다 무거워요"));
        assert_eq!(v.choice.reason_polarity(), "both_lacking");

        // 이유와 메모는 고르지 않아도 된다.
        let v = validate_vote(req(r#"{"choice":"a"}"#).unwrap()).unwrap();
        assert!(v.reasons.is_empty());
        assert_eq!(v.comment, None);
        let v = validate_vote(req(r#"{"choice":"b","comment":"   "}"#).unwrap()).unwrap();
        assert_eq!(v.comment, None);
        assert_eq!(v.choice.reason_polarity(), "chosen");
    }

    #[test]
    fn malformed_votes_are_rejected() {
        assert!(req(r#"{"choice":"c"}"#).is_err());
        assert!(req(r#"{"choice":"skip"}"#).is_err());
        assert!(req(r#"{}"#).is_err());
        assert!(req(r#"{"choice":"a","reasons":["cheap"]}"#).is_err());
        // 사용자 id 나 코디를 본문으로 받지 않는다.
        assert!(req(r#"{"choice":"a","user_id":"someone"}"#).is_err());
        assert!(req(r#"{"choice":"a","item_ids":["x"]}"#).is_err());

        let long = "가".repeat(COMMENT_MAX_CHARS + 1);
        let body = serde_json::json!({ "choice": "a", "comment": long }).to_string();
        assert!(validate_vote(req(&body).unwrap()).is_err());
        let ok = "가".repeat(COMMENT_MAX_CHARS);
        let body = serde_json::json!({ "choice": "a", "comment": ok }).to_string();
        assert!(validate_vote(req(&body).unwrap()).is_ok());
    }
}
