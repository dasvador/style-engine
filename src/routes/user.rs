use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::errors::AppError;

pub fn router() -> Router<AppState> {
    Router::new().route("/register", post(register))
}

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    display_name: Option<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    user_id: String,
    api_token: String,
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    let user_id = Uuid::new_v4().to_string();
    let token = format!("tok-{}", Uuid::new_v4().to_string().replace('-', ""));

    sqlx::query("INSERT INTO app_user (id, username, api_token, display_name) VALUES (?, ?, ?, ?)")
        .bind(&user_id)
        .bind(&body.username)
        .bind(&token)
        .bind(&body.display_name)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // 유저 프로파일 초기화
    sqlx::query("INSERT INTO user_style_profile (user_id) VALUES (?)")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(RegisterResponse {
        user_id,
        api_token: token,
    }))
}
