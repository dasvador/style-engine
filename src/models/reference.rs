use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// DB row for clothing_reference table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ClothingReference {
    pub id: String,
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    pub embedding: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Request body for creating a reference
#[derive(Debug, Deserialize)]
pub struct CreateReferenceRequest {
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
}

/// Response DTO
#[derive(Debug, Serialize)]
pub struct ReferenceResponse {
    pub id: String,
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// A matched reference with similarity score
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceMatch {
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    pub similarity: f32,
}
