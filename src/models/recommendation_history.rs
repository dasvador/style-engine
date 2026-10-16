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

/// 최근 추천과 겹치는 정도. 추천 이력이지 착용 이력이 아니다.
///
/// 옷(상의·하의·아우터)과 보조 아이템(신발·가방)을 따로 센다. 예전에는 다섯 칸 중 하나만
/// 겹쳐도 "아이템 재사용" 이 켜져, 가방 하나를 다시 든 새 상하의 조합이 같은 착장을
/// 다시 낸 것과 비슷하게 23점을 잃었다(2026-10-17 확인 — 글램 옷장은 가방이 넷뿐이다).
#[derive(Debug, Clone, Default)]
pub struct OutfitHistorySummary {
    /// 상의·하의·아우터 중 하나가 어제 추천에 있었다.
    pub item_used_yesterday: bool,
    /// 상의·하의·아우터 중 하나가 3일 안의 추천에 있었다.
    pub item_used_within_3_days: bool,
    /// 신발·가방만 3일 안의 추천과 겹친다(옷은 겹치지 않는다).
    pub accessory_used_within_3_days: bool,
    pub same_top_bottom_within_7_days: bool,
    pub same_full_outfit_within_14_days: bool,
    /// 7일 안의 추천 중 옷이 겹친 횟수.
    pub recent_usage_count_7d: i32,
}

pub fn new_history_id() -> String {
    Uuid::new_v4().to_string()
}
