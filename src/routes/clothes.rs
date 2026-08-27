use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
};
use validator::Validate;

use crate::AppState;
use crate::db::clothing_repo;
use crate::errors::AppError;
use crate::models::clothing::{
    ClothingResponse, CreateClothingRequest, ImageUploadRequest, UpdateClothingRequest,
};
use crate::models::style_vocab::Thickness;
use crate::services::llm::LlmTask;
use crate::services::prompts;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_clothing).get(list_clothing))
        .route(
            "/{id}",
            get(get_clothing)
                .put(update_clothing)
                .delete(delete_clothing),
        )
        .route(
            "/upload",
            post(upload_clothing_image).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
}

async fn create_clothing(
    State(state): State<AppState>,
    Json(body): Json<CreateClothingRequest>,
) -> Result<Json<ClothingResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let thickness = body.thickness.unwrap_or(Thickness::Medium);

    let clothing = clothing_repo::insert_clothing(
        &state.db,
        &body.name,
        &body.category,
        body.color.as_deref(),
        thickness,
        body.image_url.as_deref(),
        body.tone,
        body.saturation,
        body.style,
        body.weight,
        body.role,
        body.color_temperature.as_deref(),
        body.versatility.as_deref(),
        body.statement_level,
        body.formality_level,
    )
    .await?;

    let seasons = body.seasons.unwrap_or_default();
    if !seasons.is_empty() {
        clothing_repo::insert_seasons(&state.db, &clothing.id, &seasons).await?;
    }

    let texture_worlds = body.texture_worlds.unwrap_or_default();
    if !texture_worlds.is_empty() {
        clothing_repo::insert_texture_worlds(&state.db, &clothing.id, &texture_worlds).await?;
    }

    let all_seasons = clothing_repo::get_seasons(&state.db, &clothing.id).await?;
    let all_tw = clothing_repo::get_texture_worlds(&state.db, &clothing.id).await?;

    Ok(Json(to_response(clothing, all_seasons, all_tw)))
}

async fn list_clothing(
    State(state): State<AppState>,
) -> Result<Json<Vec<ClothingResponse>>, AppError> {
    let items = clothing_repo::list_clothing(&state.db).await?;
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let seasons = clothing_repo::get_seasons(&state.db, &item.id).await?;
        let tw = clothing_repo::get_texture_worlds(&state.db, &item.id).await?;
        result.push(to_response(item, seasons, tw));
    }
    Ok(Json(result))
}

async fn get_clothing(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ClothingResponse>, AppError> {
    let clothing = clothing_repo::get_clothing_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Clothing {} not found", id)))?;

    let seasons = clothing_repo::get_seasons(&state.db, &clothing.id).await?;
    let tw = clothing_repo::get_texture_worlds(&state.db, &clothing.id).await?;
    Ok(Json(to_response(clothing, seasons, tw)))
}

async fn update_clothing(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateClothingRequest>,
) -> Result<Json<ClothingResponse>, AppError> {
    let clothing = clothing_repo::update_clothing(
        &state.db,
        &id,
        body.name.as_deref(),
        body.category.as_deref(),
        body.color.as_deref(),
        body.thickness,
        body.image_url.as_deref(),
        body.tone,
        body.saturation,
        body.style,
        body.weight,
        body.role,
        body.color_temperature.as_deref(),
        body.versatility.as_deref(),
        body.statement_level,
        body.formality_level,
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Clothing {} not found", id)))?;

    if let Some(seasons) = body.seasons {
        clothing_repo::delete_seasons(&state.db, &id).await?;
        if !seasons.is_empty() {
            clothing_repo::insert_seasons(&state.db, &id, &seasons).await?;
        }
    }

    if let Some(texture_worlds) = body.texture_worlds {
        clothing_repo::delete_texture_worlds(&state.db, &id).await?;
        if !texture_worlds.is_empty() {
            clothing_repo::insert_texture_worlds(&state.db, &id, &texture_worlds).await?;
        }
    }

    let all_seasons = clothing_repo::get_seasons(&state.db, &clothing.id).await?;
    let all_tw = clothing_repo::get_texture_worlds(&state.db, &clothing.id).await?;
    Ok(Json(to_response(clothing, all_seasons, all_tw)))
}

async fn delete_clothing(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = clothing_repo::delete_clothing(&state.db, &id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("Clothing {} not found", id)));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn upload_clothing_image(
    State(state): State<AppState>,
    Json(body): Json<ImageUploadRequest>,
) -> Result<Json<ClothingResponse>, AppError> {
    state
        .llm
        .ensure_configured(LlmTask::VisionPass1)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    if !body.image_data.starts_with("data:image/") {
        return Err(AppError::BadRequest(
            "유효하지 않은 이미지입니다.".to_string(),
        ));
    }

    let analysis =
        prompts::analyze_clothing_image_with_rag(&state.llm, &body.image_data, &state.embedding)
            .await
            .map_err(AppError::Internal)?;

    if !analysis.is_clothing {
        let reason = analysis
            .rejection_reason
            .unwrap_or_else(|| "이미지가 의류 항목이 아닙니다.".to_string());
        return Err(AppError::BadRequest(reason));
    }

    let name = analysis
        .name
        .ok_or_else(|| AppError::BadRequest("AI가 의류 이름을 인식하지 못했습니다.".to_string()))?;
    let category = analysis
        .category
        .ok_or_else(|| AppError::BadRequest("AI가 카테고리를 인식하지 못했습니다.".to_string()))?;
    let color = analysis.color;
    let thickness = analysis.thickness.unwrap_or(Thickness::Medium);
    let seasons = analysis.seasons.unwrap_or_default();
    let texture_worlds = analysis.texture_worlds.unwrap_or_default();

    let clothing = clothing_repo::insert_clothing(
        &state.db,
        &name,
        &category,
        color.as_deref(),
        thickness,
        Some(&body.image_data),
        analysis.tone,
        analysis.saturation,
        analysis.style,
        analysis.weight,
        analysis.role,
        analysis.color_temperature.as_deref(),
        analysis.versatility.as_deref(),
        analysis.statement_level,
        analysis.formality_level,
    )
    .await?;

    if !seasons.is_empty() {
        clothing_repo::insert_seasons(&state.db, &clothing.id, &seasons).await?;
    }
    if !texture_worlds.is_empty() {
        clothing_repo::insert_texture_worlds(&state.db, &clothing.id, &texture_worlds).await?;
    }

    let all_seasons = clothing_repo::get_seasons(&state.db, &clothing.id).await?;
    let all_tw = clothing_repo::get_texture_worlds(&state.db, &clothing.id).await?;

    Ok(Json(to_response(clothing, all_seasons, all_tw)))
}

fn to_response(
    c: crate::models::clothing::Clothing,
    seasons: Vec<String>,
    texture_worlds: Vec<String>,
) -> ClothingResponse {
    ClothingResponse {
        id: c.id,
        name: c.name,
        category: c.category,
        color: c.color,
        thickness: c.thickness,
        image_url: c.image_url,
        seasons,
        tone: c.tone,
        saturation: c.saturation,
        style: c.style,
        weight: c.weight,
        role: c.role,
        color_temperature: c.color_temperature,
        versatility: c.versatility,
        statement_level: c.statement_level,
        formality_level: c.formality_level,
        texture_worlds,
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}
