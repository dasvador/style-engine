use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

// sqlx가 쿼리 결과에서 직접 구성하고 serde가 직렬화한다 — derive 경유 사용은 dead_code 분석에 잡히지 않는다.
#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct OutfitFeedback {
    pub id: String,
    pub user_id: String,
    pub feedback_type: String,
    pub reason: Option<String>,
    pub inner_name: Option<String>,
    pub outer_name: Option<String>,
    pub bottom_name: Option<String>,
    pub shoes_name: Option<String>,
    pub bag_name: Option<String>,
    pub anchor_name: Option<String>,
    pub comment: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub feedback_type: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    pub inner_name: Option<String>,
    pub outer_name: Option<String>,
    pub bottom_name: Option<String>,
    pub shoes_name: Option<String>,
    pub bag_name: Option<String>,
    pub anchor_name: Option<String>,
    pub comment: Option<String>,
}

#[allow(dead_code)] // 위와 동일 — sqlx가 채우는 행 구조체
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ItemFeedbackScore {
    pub user_id: String,
    pub item_name: String,
    pub score_adjustment: i32,
    pub feedback_count: i32,
}

#[allow(dead_code)] // 위와 동일 — sqlx가 채우는 행 구조체
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserPreferenceScore {
    pub user_id: String,
    pub reason_tag: String,
    pub score: i32,
    pub count: i32,
}
