use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::feedback::{FeedbackRequest, ItemFeedbackScore};

pub async fn insert_feedback(
    pool: &MySqlPool,
    user_id: &str,
    req: &FeedbackRequest,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO outfit_feedback (id, user_id, feedback_type, reason, inner_name, outer_name, bottom_name, shoes_name, bag_name, anchor_name, comment) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(user_id)
    .bind(&req.feedback_type)
    .bind(&req.reason)
    .bind(&req.inner_name)
    .bind(&req.outer_name)
    .bind(&req.bottom_name)
    .bind(&req.shoes_name)
    .bind(&req.bag_name)
    .bind(&req.anchor_name)
    .bind(&req.comment)
    .execute(pool)
    .await?;

    // 아이템별 보정 점수 업데이트
    let delta = match req.feedback_type.as_str() {
        "like" | "worn" | "saved" => 2,
        "dislike" => -3,
        "skipped" => -1,
        _ => 0,
    };

    let items = [
        &req.inner_name,
        &req.outer_name,
        &req.bottom_name,
        &req.shoes_name,
        &req.bag_name,
    ];

    for item in items.iter().filter_map(|i| i.as_ref()) {
        if !item.is_empty() {
            sqlx::query(
                "INSERT INTO item_feedback_score (user_id, item_name, score_adjustment, feedback_count) VALUES (?, ?, ?, 1) ON DUPLICATE KEY UPDATE score_adjustment = score_adjustment + ?, feedback_count = feedback_count + 1"
            )
            .bind(user_id)
            .bind(item)
            .bind(delta)
            .bind(delta)
            .execute(pool)
            .await?;
        }
    }

    Ok(id)
}

pub async fn get_item_adjustments(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<ItemFeedbackScore>, sqlx::Error> {
    sqlx::query_as::<_, ItemFeedbackScore>(
        "SELECT user_id, item_name, score_adjustment, feedback_count FROM item_feedback_score WHERE user_id = ? AND score_adjustment != 0"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}
