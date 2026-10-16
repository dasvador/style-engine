use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;

/// 남겨 둔 코디 한 장.
///
/// 사진은 `image_path` 에 붙여 둔다. 처음 붙을 때는 `outfit_image` 캐시에서
/// `image_prompt` 의 해시로 찾는다.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LookRow {
    pub id: String,
    pub mood_key: Option<String>,
    pub title: String,
    pub weather_summary: Option<String>,
    pub reason: Option<String>,
    pub recommendation: Option<String>,
    pub image_prompt: String,
    /// 저장할 때 받은 아이템 목록을 그대로 돌려준다.
    #[sqlx(json)]
    pub outfit_json: serde_json::Value,
    pub liked: bool,
    pub worn: bool,
    pub created_at: chrono::NaiveDateTime,
    /// 이미 만들어 둔 그림의 경로. 없으면 아직 생성되지 않은 것이다.
    ///
    /// 화면이 저장된 룩마다 이미지를 요청하면, 캐시에 없는 것은 그 자리에서
    /// 생성되기 시작한다 — 모아 보는 화면을 여는 것만으로 돈이 나간다.
    /// 그래서 서버가 룩에 붙은 사진(없으면 캐시)을 찾아 있는 것만 내려 준다.
    pub image_path: Option<String>,
    /// 이 코디가 나온 가장 최근 홈 추천과, 그 묶음에서 "제일" 로 골랐는지. 새로고침
    /// 뒤에도 홈이 "제일" 버튼을 다시 그리고 취소할 수 있게 돌려준다.
    pub request_id: Option<String>,
    pub best: i64,
}

#[derive(Debug, Deserialize)]
pub struct SaveLookRequest {
    pub mood_key: Option<String>,
    pub title: String,
    pub weather_summary: Option<String>,
    pub reason: Option<String>,
    pub recommendation: Option<String>,
    pub image_prompt: String,
    pub outfit_json: serde_json::Value,
    /// 홈 추천 한 번을 가리키는 값. 있으면 `look_signal` 에 이 요청의 코디로 남긴다.
    /// 상담 화면처럼 비교 묶음이 없는 저장은 보내지 않는다.
    #[serde(default)]
    pub request_id: Option<String>,
    /// 응답 안의 순서(0부터).
    #[serde(default)]
    pub position: Option<u8>,
    #[serde(default)]
    pub gender: Option<String>,
    /// 홈 추천이 이 코디에 매긴 점수 내역과 shadow 점수. 그대로 `look_signal` 에 남긴다.
    #[serde(default)]
    pub scoring: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct MarkRequest {
    pub liked: Option<bool>,
    pub worn: Option<bool>,
}

/// 추천 묶음 하나에서 "이번엔 이게 제일" 로 고른 코디.
#[derive(Debug, Deserialize)]
pub struct BestRequest {
    pub request_id: String,
}

/// 지우는 이유. 쌍을 셀 때 코디가 별로라 지운 것만 진 쪽으로 넣는다.
#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    pub reason: Option<String>,
}

const DELETE_REASONS: [&str; 3] = ["outfit", "photo", "cleanup"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_looks).post(save_look))
        .route("/{id}", patch(mark_look).delete(delete_look))
        .route("/{id}/best", post(mark_best).delete(unmark_best))
}

/// 최근에 만든 코디부터. 화면이 한 번에 들고 있을 만큼만 준다.
async fn list_looks(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<LookRow>>, AppError> {
    let mut rows = sqlx::query_as::<_, LookRow>(
        "SELECT id, mood_key, title, weather_summary, reason, recommendation, \
         image_prompt, outfit_json, liked, worn, created_at, image_path, \
         (SELECT s.request_id FROM look_signal s WHERE s.look_id = lookbook_look.id \
           ORDER BY s.created_at DESC, s.id DESC LIMIT 1) AS request_id, \
         CAST(COALESCE((SELECT s.best FROM look_signal s WHERE s.look_id = lookbook_look.id \
           ORDER BY s.created_at DESC, s.id DESC LIMIT 1), 0) AS SIGNED) AS best \
         FROM lookbook_look WHERE user_id = ? ORDER BY created_at DESC LIMIT 60",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    // 담아 둔 사진이 없는 룩만 이미지 캐시에서 찾아 붙이고, 찾으면 룩에 적어 둔다.
    // 생성은 하지 않는다 — 모아 보는 화면을 여는 것만으로 이미지가 만들어지면 비용이
    // 조용히 새어 나간다.
    for row in &mut rows {
        if row.image_path.is_some() {
            continue;
        }
        row.image_path =
            cached_image_path(&state.db, &row.image_prompt, row.mood_key.as_deref()).await;
        if let Some(path) = &row.image_path {
            pin_image(&state.db, &row.image_prompt, row.mood_key.as_deref(), path).await;
        }
    }

    Ok(Json(rows))
}

#[derive(Serialize)]
struct SaveResponse {
    id: String,
}

/// 같은 코디를 다시 만들어도 한 행만 남긴다 — 이미지도 같은 캐시를 쓴다.
async fn save_look(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SaveLookRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO lookbook_look \
         (id, user_id, mood_key, title, weather_summary, reason, recommendation, \
          image_prompt, outfit_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE title = VALUES(title), \
          weather_summary = VALUES(weather_summary), reason = VALUES(reason), \
          recommendation = VALUES(recommendation)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&body.mood_key)
    .bind(&body.title)
    .bind(&body.weather_summary)
    .bind(&body.reason)
    .bind(&body.recommendation)
    .bind(&body.image_prompt)
    .bind(&body.outfit_json)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    // 이미 있던 코디면 INSERT 가 아니라 UPDATE 였으므로 기존 id 를 돌려준다.
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM lookbook_look WHERE user_id = ? AND image_prompt = ? LIMIT 1",
    )
    .bind(&auth.user_id)
    .bind(&body.image_prompt)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let id = existing.map(|r| r.0).unwrap_or(id);

    // 담는 순간 보이던 사진을 룩에 붙여 둔다. 아직 사진이 없으면 처음 만들어질 때
    // `chat::generate_image` 가 붙인다.
    if let Some(path) =
        cached_image_path(&state.db, &body.image_prompt, body.mood_key.as_deref()).await
    {
        pin_image(
            &state.db,
            &body.image_prompt,
            body.mood_key.as_deref(),
            &path,
        )
        .await;
    }

    // 비교 묶음 기록은 곁가지다. 실패해도 룩북 저장은 성공으로 돌려준다.
    if let Some(request_id) = body.request_id.as_deref().filter(|r| !r.is_empty()) {
        let recorded = sqlx::query(
            "INSERT IGNORE INTO look_signal \
             (user_id, request_id, look_id, position, gender, mood_key, weather_summary, \
              outfit_json, scoring_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&auth.user_id)
        .bind(request_id)
        .bind(&id)
        .bind(body.position.unwrap_or(0))
        .bind(&body.gender)
        .bind(&body.mood_key)
        .bind(&body.weather_summary)
        .bind(&body.outfit_json)
        .bind(&body.scoring)
        .execute(&state.db)
        .await;
        if let Err(e) = recorded {
            tracing::warn!("look_signal 기록 실패: {e}");
        }
    }

    Ok(Json(SaveResponse { id }))
}

/// 지금 프롬프트로 만든 사진이 캐시에 있으면 그 경로.
async fn cached_image_path(
    db: &sqlx::MySqlPool,
    image_prompt: &str,
    mood_key: Option<&str>,
) -> Option<String> {
    let genre = mood_key.and_then(crate::models::style_vocab::StyleGenre::from_alias)?;
    let (outfit_hash, prompt_hash, _, _) =
        crate::routes::chat::image_cache_keys(image_prompt, genre);
    sqlx::query_as::<_, (Option<String>,)>(
        "SELECT image_path FROM outfit_image WHERE outfit_hash = ? AND prompt_hash = ? LIMIT 1",
    )
    .bind(&outfit_hash)
    .bind(&prompt_hash)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .and_then(|r| r.0)
}

/// 담아 둔 룩에 붙은 사진. 이미지 프롬프트가 바뀌어도 이 사진을 계속 보여 준다 —
/// 캐시 키에는 프롬프트 전체의 해시가 들어가서, 문구를 고치면 예전 사진과의 짝이
/// 끊긴다(마이그레이션 `20261016000001`).
pub async fn pinned_image(
    db: &sqlx::MySqlPool,
    image_prompt: &str,
    mood_key: Option<&str>,
) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT image_path FROM lookbook_look \
         WHERE image_prompt = ? AND mood_key <=> ? AND image_path IS NOT NULL LIMIT 1",
    )
    .bind(image_prompt)
    .bind(mood_key)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .map(|r| r.0)
}

/// 사진이 아직 없는 룩에만 붙인다. 한 번 붙은 사진은 바꾸지 않는다.
pub async fn pin_image(
    db: &sqlx::MySqlPool,
    image_prompt: &str,
    mood_key: Option<&str>,
    path: &str,
) {
    let _ = sqlx::query(
        "UPDATE lookbook_look SET image_path = ? \
         WHERE image_prompt = ? AND mood_key <=> ? AND image_path IS NULL",
    )
    .bind(path)
    .bind(image_prompt)
    .bind(mood_key)
    .execute(db)
    .await;
}

/// '마음에 들어요' / '오늘 입을래요' 를 켜거나 끈다. 보내지 않은 쪽은 그대로 둔다.
///
/// 끄기는 잘못 누른 것을 바로잡는 용도다(2026-10-16 사용자 요청 — 한 번 누르면 되돌릴
/// 수 없었다). 그래서 끌 때는 그때 남긴 피드백도 되돌린다. 화면은 켤 때 피드백을 따로
/// 보내고(`/feedback`), 그 피드백은 아이템 점수에 더해져 다음 추천에 반영되기 때문이다.
async fn mark_look(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<MarkRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    let current: Option<(bool, bool, serde_json::Value)> = sqlx::query_as(
        "SELECT liked, worn, outfit_json FROM lookbook_look WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    let Some((was_liked, was_worn, outfit)) = current else {
        return Err(AppError::NotFound(
            "이 코디를 찾을 수 없습니다.".to_string(),
        ));
    };

    for (flag, was, column, feedback_type) in [
        (body.liked, was_liked, "liked", "like"),
        (body.worn, was_worn, "worn", "worn"),
    ] {
        let Some(on) = flag else { continue };
        if on == was {
            continue;
        }
        // 열 이름은 위의 고정된 두 값뿐이다.
        sqlx::query(&format!(
            "UPDATE lookbook_look SET {column} = ? WHERE id = ? AND user_id = ?"
        ))
        .bind(on)
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // 같은 코디가 나온 요청 모두에 표시한다 — 좋아요는 그 코디에 대한 판단이다.
        let _ = sqlx::query(&format!(
            "UPDATE look_signal SET {column} = ?, {column}_at = IF(?, NOW(), NULL) \
             WHERE look_id = ? AND user_id = ?"
        ))
        .bind(on)
        .bind(on)
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await;

        if !on {
            let names = feedback_names(&outfit);
            let undone = crate::db::feedback_repo::undo_latest_feedback(
                &state.db,
                &auth.user_id,
                feedback_type,
                names.each_ref().map(|n| n.as_deref()),
            )
            .await;
            if let Err(e) = undone {
                tracing::warn!("{feedback_type} 피드백 되돌리기 실패: {e}");
            }
        }
    }

    Ok(Json(SaveResponse { id }))
}

/// 홈이 피드백을 보낼 때 쓰는 슬롯 순서(상의·아우터·하의·신발·가방)로 아이템 이름을 꺼낸다.
fn feedback_names(outfit: &serde_json::Value) -> [Option<String>; 5] {
    let name = |category: &str| {
        outfit.as_array()?.iter().find_map(|o| {
            (o.get("category")?.as_str()? == category)
                .then(|| o.get("name")?.as_str().map(str::to_string))
                .flatten()
        })
    };
    ["상의", "아우터", "하의", "신발", "가방"].map(name)
}

/// 추천 묶음 하나에서 이 코디를 "제일" 로 고른다. 다시 고르면 바뀐다 — 묶음마다
/// 하나만 남는다.
async fn mark_best(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<BestRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    let done = sqlx::query(
        "UPDATE look_signal SET \
           best_at = IF(look_id = ?, NOW(), NULL), best = (look_id = ?) \
         WHERE request_id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&id)
    .bind(&body.request_id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "이 추천 묶음을 찾을 수 없습니다.".to_string(),
        ));
    }
    Ok(Json(SaveResponse { id }))
}

/// "제일" 선택을 취소한다. 그 묶음에는 고른 코디가 없게 된다.
async fn unmark_best(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(body): Query<BestRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    sqlx::query(
        "UPDATE look_signal SET best = 0, best_at = NULL \
         WHERE look_id = ? AND request_id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&body.request_id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(SaveResponse { id }))
}

/// 룩북에서 코디 한 장을 지운다. 내 것만 지워진다.
///
/// 그림(`outfit_image`)은 지우지 않는다 — 다른 룩이나 같은 코디의 다음 요청이 같은
/// 캐시를 쓰고, 다시 만들면 돈이 든다.
///
/// 지운 사실은 `look_signal` 에 남긴다. 룩북 행은 사라지지만, 일부러 지웠다는 것은
/// 같은 요청의 다른 코디보다 못하다는 가장 분명한 신호다.
async fn delete_look(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Result<Json<SaveResponse>, AppError> {
    let reason = match q.reason.as_deref() {
        None => None,
        Some(r) if DELETE_REASONS.contains(&r) => Some(r),
        Some(r) => {
            return Err(AppError::BadRequest(format!(
                "지우는 이유는 {} 중 하나여야 합니다: {r}",
                DELETE_REASONS.join(", ")
            )));
        }
    };

    // 행이 사라지기 전에 사진 경로를 읽어 둔다 — 아래에서 `look_signal` 로 옮긴다.
    let image_path: Option<String> = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT image_path FROM lookbook_look WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .and_then(|r| r.0);

    let done = sqlx::query("DELETE FROM lookbook_look WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "이 코디를 찾을 수 없습니다.".to_string(),
        ));
    }
    let _ = sqlx::query(
        "UPDATE look_signal SET deleted = 1, deleted_at = NOW(), delete_reason = ?, \
           image_path = COALESCE(image_path, ?) \
         WHERE look_id = ? AND user_id = ?",
    )
    .bind(reason)
    .bind(&image_path)
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await;
    Ok(Json(SaveResponse { id }))
}
