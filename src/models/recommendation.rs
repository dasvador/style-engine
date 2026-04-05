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

/// Shape we expect from OpenAI JSON response (single candidate)
#[derive(Debug, Deserialize)]
pub struct AiRecommendation {
    pub recommendation: String,
    pub outfit: Vec<AiOutfitItem>,
    pub weather_summary: String,
    pub tips: Vec<String>,
}

/// Multi-candidate response from OpenAI
#[derive(Debug, Deserialize)]
pub struct AiMultiRecommendation {
    pub candidates: Vec<AiRecommendation>,
}

/// 후보 코디 (style_engine 점수 + 리센시 페널티 + 다양성 보너스)
#[derive(Debug, Clone)]
pub struct OutfitCandidate {
    pub top_id: Option<String>,
    pub bottom_id: Option<String>,
    pub outer_id: Option<String>,
    pub shoes_id: Option<String>,
    pub bag_id: Option<String>,

    pub style_score: i32,
    pub recency_penalty: i32,
    pub diversity_bonus: i32,
    pub final_score: i32,

    /// 원본 AI 추천 인덱스 (candidates 배열 내 위치)
    pub ai_candidate_index: usize,
}

// ─── 3-모드 추천 시스템 ───

#[derive(Debug, Clone, Serialize)]
pub struct ScoringDetail {
    pub style_score: i32,
    pub recency_penalty: i32,
    pub diversity_bonus: i32,
    pub dormant_bonus: i32,
    pub final_score: i32,
}

#[derive(Debug, Serialize)]
pub struct ModeRecommendation {
    pub mode: String,
    pub mode_label: String,
    pub mode_description: String,
    pub outfit: Vec<OutfitItem>,
    pub recommendation: String,
    pub weather_summary: String,
    pub tips: Vec<String>,
    pub score: i32,
    pub verdict: String,
    pub reason: String,
    pub revival_items: Vec<String>,
    pub scoring_detail: ScoringDetail,
}

#[derive(Debug, Serialize)]
pub struct MultiModeRecommendationResponse {
    pub modes: Vec<ModeRecommendation>,
    pub weather_summary: String,
}

impl OutfitCandidate {
    pub fn new(
        top_id: Option<String>,
        bottom_id: Option<String>,
        outer_id: Option<String>,
        shoes_id: Option<String>,
        bag_id: Option<String>,
        ai_candidate_index: usize,
    ) -> Self {
        Self {
            top_id,
            bottom_id,
            outer_id,
            shoes_id,
            bag_id,
            style_score: 0,
            recency_penalty: 0,
            diversity_bonus: 0,
            final_score: 0,
            ai_candidate_index,
        }
    }
}
