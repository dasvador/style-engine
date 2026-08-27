use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::models::style_vocab::{Role, Saturation, Style, Thickness, Tone, Weight};

/// DB row for clothing table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Clothing {
    pub id: String,
    pub name: String,
    pub category: String,
    pub gender: Option<String>,
    pub style_mood: Option<String>,
    pub color: Option<String>,
    pub thickness: Thickness,
    pub image_url: Option<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub visual_weight: Option<String>,
    pub texture_depth: Option<String>,
    pub visual_weight_v2: Option<i8>,
    pub texture_depth_v2: Option<i8>,
    pub grounding_score: Option<i8>,
    pub shadow_tone: Option<String>,
    pub silhouette_volume: Option<String>,
    pub material_primary: Option<String>,
    pub sub_category: Option<String>,
    pub floating_score: Option<i8>,
    pub strong_style_score: Option<i8>,
    pub texture_keywords: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Request body for creating clothing
#[derive(Debug, Deserialize, Validate)]
pub struct CreateClothingRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1, max = 50))]
    pub category: String,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub image_url: Option<String>,
    pub seasons: Option<Vec<String>>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
}

/// Request body for updating clothing
#[derive(Debug, Deserialize)]
pub struct UpdateClothingRequest {
    pub name: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub image_url: Option<String>,
    pub seasons: Option<Vec<String>>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
}

/// Response from OpenAI Vision API clothing analysis
#[derive(Debug, Deserialize)]
pub struct VisionAnalysisResult {
    pub is_clothing: bool,
    pub name: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub seasons: Option<Vec<String>>,
    pub rejection_reason: Option<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
}

/// Request body for image-based clothing upload
#[derive(Debug, Deserialize)]
pub struct ImageUploadRequest {
    pub image_data: String,
}

/// Pass 1 result from Vision API: simple description of the clothing item
#[derive(Debug, Deserialize)]
pub struct Pass1Result {
    pub description: String,
}

/// Response DTO with seasons included
#[derive(Debug, Serialize)]
pub struct ClothingResponse {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: Option<String>,
    pub thickness: Thickness,
    pub image_url: Option<String>,
    pub seasons: Vec<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Vec<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
