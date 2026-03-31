use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// DB row for region_setting table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct RegionSetting {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub updated_at: NaiveDateTime,
}

/// Request body for upserting region
#[derive(Debug, Deserialize, Validate)]
pub struct UpsertRegionRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}
