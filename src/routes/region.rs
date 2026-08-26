use axum::{Json, Router, extract::State, routing::get};
use validator::Validate;

use crate::AppState;
use crate::db::region_repo;
use crate::errors::AppError;
use crate::models::region::{RegionSetting, UpsertRegionRequest};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_region).put(upsert_region))
}

async fn get_region(State(state): State<AppState>) -> Result<Json<RegionSetting>, AppError> {
    let region = region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("No region configured".to_string()))?;
    Ok(Json(region))
}

async fn upsert_region(
    State(state): State<AppState>,
    Json(body): Json<UpsertRegionRequest>,
) -> Result<Json<RegionSetting>, AppError> {
    body.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let region =
        region_repo::upsert_region(&state.db, &body.name, body.latitude, body.longitude).await?;
    Ok(Json(region))
}
