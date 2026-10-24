use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/me", get(me))
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

    // 새 계정만 만든다. 이미 있는 username 으로는 그 계정의 토큰을 받을 수 없다(409).
    let inserted = sqlx::query(
        "INSERT INTO app_user (id, username, api_token, display_name) VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&body.username)
    .bind(&token)
    .bind(&body.display_name)
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            return Err(AppError::Conflict("이미 쓰고 있는 이름이에요.".to_string()));
        }
        Err(e) => return Err(AppError::Internal(e.into())),
    }

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

#[derive(Serialize, sqlx::FromRow)]
struct MeResponse {
    user_id: String,
    username: String,
    display_name: Option<String>,
}

/// 나. 토큰 값은 돌려주지 않는다.
async fn me(State(state): State<AppState>, auth: AuthUser) -> Result<Json<MeResponse>, AppError> {
    sqlx::query_as::<_, MeResponse>(
        "SELECT id AS user_id, username, display_name FROM app_user WHERE id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or_else(|| AppError::NotFound("사용자를 찾을 수 없어요.".to_string()))
}
