use axum::{extract::State, routing::post, Json, Router};

use crate::db::{clothing_repo, region_repo};
use crate::errors::AppError;
use crate::models::clothing::Clothing;
use crate::models::recommendation::{
    OutfitItem, RecommendationRequest, RecommendationResponse,
};
use crate::services::{openai, weather as weather_service};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(get_recommendation))
}

async fn get_recommendation(
    State(state): State<AppState>,
    Json(body): Json<RecommendationRequest>,
) -> Result<Json<RecommendationResponse>, AppError> {
    if state.openai_api_key.is_empty() || state.openai_api_key == "sk-your-key-here" {
        return Err(AppError::BadRequest(
            "OPENAI_API_KEY is not configured".to_string(),
        ));
    }

    // 1. Get region
    let region = region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No region configured. Set a region first.".to_string())
        })?;

    // 2. Fetch weather
    let weather = weather_service::fetch_weather(
        &state.http_client,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(AppError::Internal)?;

    // 3. Get user's clothes
    let clothes = clothing_repo::list_clothing(&state.db).await?;
    let clothes_names: Vec<String> = clothes
        .iter()
        .map(|c| format!("{} ({})", c.name, c.category))
        .collect();

    // 4. Call OpenAI
    let ai_result = openai::get_outfit_recommendation(
        &state.http_client,
        &state.openai_api_key,
        &weather,
        &clothes_names,
        body.occasion.as_deref(),
        body.style_preference.as_deref(),
    )
    .await
    .map_err(AppError::Internal)?;

    // 5. Match AI-recommended items to DB records for image_url
    let outfit: Vec<OutfitItem> = ai_result
        .outfit
        .into_iter()
        .map(|ai_item| {
            let image_url = find_matching_clothing(&clothes, &ai_item.name)
                .and_then(|c| c.image_url.clone());
            OutfitItem {
                category: ai_item.category,
                name: ai_item.name,
                reason: ai_item.reason,
                image_url,
            }
        })
        .collect();

    Ok(Json(RecommendationResponse {
        recommendation: ai_result.recommendation,
        outfit,
        weather_summary: ai_result.weather_summary,
        tips: ai_result.tips,
    }))
}

/// Find a matching clothing record by name.
/// Tries exact match first, then substring contains.
fn find_matching_clothing<'a>(clothes: &'a [Clothing], name: &str) -> Option<&'a Clothing> {
    // 1. Exact match
    if let Some(c) = clothes.iter().find(|c| c.name == name) {
        return Some(c);
    }
    // 2. AI name contains DB name, or DB name contains AI name
    clothes.iter().find(|c| name.contains(&c.name) || c.name.contains(name))
}
