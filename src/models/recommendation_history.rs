use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OutfitRecommendationHistory {
    pub id: String,
    pub user_id: String,
    pub top_id: Option<String>,
    pub bottom_id: Option<String>,
    pub outer_id: Option<String>,
    pub shoes_id: Option<String>,
    pub bag_id: Option<String>,
    pub recommended_at: NaiveDateTime,
}

#[derive(Debug, Clone, Default)]
pub struct OutfitHistorySummary {
    pub item_used_yesterday: bool,
    pub item_used_within_3_days: bool,
    pub same_top_bottom_within_7_days: bool,
    pub same_full_outfit_within_14_days: bool,
    pub recent_usage_count_7d: i32,
}

pub fn new_history_id() -> String {
    Uuid::new_v4().to_string()
}
