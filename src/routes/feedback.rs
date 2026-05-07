use axum::{extract::State, routing::post, Json, Router};
use serde::Serialize;

use crate::db::feedback_repo;
use crate::errors::AppError;
use crate::models::feedback::FeedbackRequest;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(submit_feedback))
}

#[derive(Serialize)]
struct FeedbackResponse {
    id: String,
    status: String,
}

async fn submit_feedback(
    State(state): State<AppState>,
    Json(body): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, AppError> {
    let user_id = "default"; // TODO: 멀티유저 시 로그인 유저로 교체

    let id = feedback_repo::insert_feedback(&state.db, user_id, &body)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(FeedbackResponse {
        id,
        status: "saved".to_string(),
    }))
}
