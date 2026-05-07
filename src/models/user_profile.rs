use serde::Serialize;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UserStyleProfile {
    pub user_id: String,
    pub height_cm: Option<i32>,
    pub weight_kg: Option<i32>,
    pub upper_body: Option<String>,
    pub calves: Option<String>,
    pub preferred_fit: Option<String>,
    pub needs_grounded_shoes: bool,
    pub prefers_weighted_bag: bool,
}
