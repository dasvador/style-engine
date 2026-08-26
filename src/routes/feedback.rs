use axum::{Json, Router, extract::State, routing::post};
use serde::Serialize;

use crate::AppState;
use crate::db::feedback_repo;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::feedback::FeedbackRequest;

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
    auth: AuthUser,
    Json(body): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, AppError> {
    let user_id = &auth.user_id;

    let id = feedback_repo::insert_feedback(&state.db, user_id, &body)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(FeedbackResponse {
        id,
        status: "saved".to_string(),
    }))
}
