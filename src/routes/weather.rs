use axum::{extract::State, routing::get, Json, Router};

use crate::db::region_repo;
use crate::errors::AppError;
use crate::models::weather::WeatherResponse;
use crate::services::weather as weather_service;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_weather))
}

async fn get_weather(
    State(state): State<AppState>,
) -> Result<Json<WeatherResponse>, AppError> {
    let region = region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("No region configured. Set a region first.".to_string()))?;

    let current = weather_service::fetch_weather(
        &state.http_client,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(AppError::Internal)?;

    Ok(Json(WeatherResponse {
        region_name: region.name,
        latitude: region.latitude,
        longitude: region.longitude,
        current,
    }))
}
