use serde::{Deserialize, Serialize};

/// Request body for recommendation endpoint
#[derive(Debug, Deserialize)]
pub struct RecommendationRequest {
    pub occasion: Option<String>,
    pub style_preference: Option<String>,
}

/// Response DTO for recommendation (to client)
#[derive(Debug, Serialize)]
pub struct RecommendationResponse {
    pub recommendation: String,
    pub outfit: Vec<OutfitItem>,
    pub weather_summary: String,
    pub tips: Vec<String>,
}

/// Outfit item returned to client — includes image_url
#[derive(Debug, Serialize)]
pub struct OutfitItem {
    pub category: String,
    pub name: String,
    pub reason: String,
    pub image_url: Option<String>,
}

/// Outfit item from OpenAI response (no image_url)
#[derive(Debug, Deserialize)]
pub struct AiOutfitItem {
    pub category: String,
    pub name: String,
    pub reason: String,
}

/// Shape we expect from OpenAI JSON response
#[derive(Debug, Deserialize)]
pub struct AiRecommendation {
    pub recommendation: String,
    pub outfit: Vec<AiOutfitItem>,
    pub weather_summary: String,
    pub tips: Vec<String>,
}
