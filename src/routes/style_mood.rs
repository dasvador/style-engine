use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_moods))
}

#[derive(Debug, Deserialize)]
struct MoodQuery {
    gender: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct StyleMood {
    gender: String,
    mood_key: String,
    mood_label: String,
    description: Option<String>,
}

async fn list_moods(
    State(state): State<AppState>,
    Query(q): Query<MoodQuery>,
) -> Result<Json<Vec<StyleMood>>, AppError> {
    // 프런트는 항상 gender 를 붙여 호출한다. 이 기본값은 파라미터 없이 부른
    // 경우에만 쓰이며, 앱의 기본 성별과 같아야 한다.
    let gender = q.gender.unwrap_or_else(|| "female".to_string());

    let moods = sqlx::query_as::<_, StyleMood>(
        "SELECT gender, mood_key, mood_label, description FROM style_mood WHERE gender = ? OR gender = 'unisex' ORDER BY sort_order"
    )
    .bind(&gender)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(moods))
}
