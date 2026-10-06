use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;

/// 남겨 둔 코디 한 장.
///
/// 이미지는 `outfit_image` 가 `image_prompt` 의 해시로 들고 있으므로 여기에는
/// 담지 않는다. 화면이 같은 문자열로 이미지를 다시 요청하면 캐시에 걸린다.
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
    /// 그래서 서버가 캐시를 조회해 있는 것만 내려 준다.
    #[sqlx(default)]
    pub image_path: Option<String>,
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
}

#[derive(Debug, Deserialize)]
pub struct MarkRequest {
    pub liked: Option<bool>,
    pub worn: Option<bool>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_looks).post(save_look))
        .route("/{id}", patch(mark_look))
}

/// 최근에 만든 코디부터. 화면이 한 번에 들고 있을 만큼만 준다.
async fn list_looks(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<LookRow>>, AppError> {
    let mut rows = sqlx::query_as::<_, LookRow>(
        "SELECT id, mood_key, title, weather_summary, reason, recommendation, \
         image_prompt, outfit_json, liked, worn, created_at \
         FROM lookbook_look WHERE user_id = ? ORDER BY created_at DESC LIMIT 60",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    // 이미 만들어 둔 그림을 붙여 준다. 생성은 하지 않는다 — 모아 보는 화면을 여는
    // 것만으로 이미지가 만들어지면 비용이 조용히 새어 나간다.
    for row in &mut rows {
        let Some(genre) = row
            .mood_key
            .as_deref()
            .and_then(crate::models::style_vocab::StyleGenre::from_alias)
        else {
            continue;
        };
        let (outfit_hash, prompt_hash, _, _) =
            crate::routes::chat::image_cache_keys(&row.image_prompt, genre);
        row.image_path = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT image_path FROM outfit_image \
             WHERE outfit_hash = ? AND prompt_hash = ? LIMIT 1",
        )
        .bind(&outfit_hash)
        .bind(&prompt_hash)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.0);
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

    Ok(Json(SaveResponse {
        id: existing.map(|r| r.0).unwrap_or(id),
    }))
}

/// '마음에 들어요' / '오늘 입을래요' 표시. 한 번 켜진 것은 꺼지지 않는다 —
/// 취향 신호를 지우는 화면이 아직 없고, 실수로 덮어쓰는 편이 더 나쁘다.
async fn mark_look(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<MarkRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    sqlx::query(
        "UPDATE lookbook_look SET liked = liked | ?, worn = worn | ? \
         WHERE id = ? AND user_id = ?",
    )
    .bind(body.liked.unwrap_or(false))
    .bind(body.worn.unwrap_or(false))
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(SaveResponse { id }))
}
