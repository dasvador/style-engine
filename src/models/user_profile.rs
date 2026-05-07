use serde::Serialize;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UserStyleProfile {
    pub user_id: String,
    // 체형
    pub height_cm: Option<i32>,
    pub weight_kg: Option<i32>,
    pub upper_body: Option<String>,
    pub calves: Option<String>,
    pub preferred_fit: Option<String>,
    pub leg_length: Option<String>,
    pub preferred_top_size: Option<String>,
    // 밸런스 룰
    pub needs_grounded_shoes: bool,
    pub prefers_weighted_bag: bool,
    pub low_profile_only_occasional: bool,
    pub medium_volume_runner_bonus: bool,
    pub denim_bridge_bonus: bool,
    // 취향: 선호
    pub likes_texture_depth: bool,
    pub likes_melange: bool,
    pub likes_suede: bool,
    pub likes_washed_denim: bool,
    pub likes_mocha_brown: bool,
    pub likes_heather_gray: bool,
    // 취향: 비선호
    pub dislikes_flat_beige: bool,
    pub dislikes_military_cosplay: bool,
    pub dislikes_bright_colors: bool,
    // 라이프스타일
    pub commute: Option<String>,
    pub walking_amount: Option<String>,
    pub comfort_priority: Option<String>,
}
