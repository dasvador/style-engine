use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};

use crate::db::reference_repo;
use crate::errors::AppError;
use crate::models::reference::{CreateReferenceRequest, ReferenceResponse};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_references).post(create_reference))
        .route(
            "/{id}",
            get(get_reference).put(update_reference).delete(delete_reference),
        )
}

async fn create_reference(
    State(state): State<AppState>,
    Json(body): Json<CreateReferenceRequest>,
) -> Result<Json<ReferenceResponse>, AppError> {
    // Embed the description
    let embedding = state
        .embedding
        .embed_text(&body.description)
        .await
        .map_err(|e| AppError::Internal(e))?;
    let emb_json = serde_json::to_value(&embedding)
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let reference = reference_repo::insert_reference(
        &state.db,
        &body.name,
        body.era.as_deref(),
        body.style.as_deref(),
        &body.description,
        Some(&emb_json),
    )
    .await?;

    // Reload cache after adding new reference
    state
        .embedding
        .load_cache(&state.db)
        .await
        .map_err(AppError::Internal)?;

    Ok(Json(to_response(reference)))
}

async fn list_references(
    State(state): State<AppState>,
) -> Result<Json<Vec<ReferenceResponse>>, AppError> {
    let refs = reference_repo::list_references(&state.db).await?;
    let result: Vec<ReferenceResponse> = refs.into_iter().map(to_response).collect();
    Ok(Json(result))
}

async fn get_reference(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ReferenceResponse>, AppError> {
    let reference = reference_repo::get_reference_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Reference {} not found", id)))?;
    Ok(Json(to_response(reference)))
}

async fn update_reference(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateReferenceRequest>,
) -> Result<Json<ReferenceResponse>, AppError> {
    let embedding = state
        .embedding
        .embed_text(&body.description)
        .await
        .map_err(|e| AppError::Internal(e))?;
    let emb_json = serde_json::to_value(&embedding)
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let reference = reference_repo::update_reference(
        &state.db,
        &id,
        Some(&body.name),
        body.era.as_deref(),
        body.style.as_deref(),
        Some(&body.description),
        Some(&emb_json),
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Reference {} not found", id)))?;

    // Reload cache
    state
        .embedding
        .load_cache(&state.db)
        .await
        .map_err(AppError::Internal)?;

    Ok(Json(to_response(reference)))
}

async fn delete_reference(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = reference_repo::delete_reference(&state.db, &id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("Reference {} not found", id)));
    }

    // Reload cache
    state
        .embedding
        .load_cache(&state.db)
        .await
        .map_err(AppError::Internal)?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn to_response(r: crate::models::reference::ClothingReference) -> ReferenceResponse {
    ReferenceResponse {
        id: r.id,
        name: r.name,
        era: r.era,
        style: r.style,
        description: r.description,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
