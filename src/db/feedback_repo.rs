use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::feedback::{FeedbackRequest, ItemFeedbackScore, UserPreferenceScore};

pub async fn insert_feedback(
    pool: &MySqlPool,
    user_id: &str,
    req: &FeedbackRequest,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let reason_str = if req.reasons.is_empty() { None } else { Some(req.reasons.join(",")) };

    sqlx::query(
        "INSERT INTO outfit_feedback (id, user_id, feedback_type, reason, inner_name, outer_name, bottom_name, shoes_name, bag_name, anchor_name, comment) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(user_id)
    .bind(&req.feedback_type)
    .bind(&reason_str)
    .bind(&req.inner_name)
    .bind(&req.outer_name)
    .bind(&req.bottom_name)
    .bind(&req.shoes_name)
    .bind(&req.bag_name)
    .bind(&req.anchor_name)
    .bind(&req.comment)
    .execute(pool)
    .await?;

    let polarity = match req.feedback_type.as_str() {
        "like" | "worn" | "saved" => "positive",
        _ => "negative",
    };

    // Layer 1: item-level (작게 — ±1)
    let item_delta = match req.feedback_type.as_str() {
        "like" | "worn" | "saved" => 1,
        "dislike" => -1,
        _ => 0,
    };
    let items = [
        &req.inner_name, &req.outer_name, &req.bottom_name,
        &req.shoes_name, &req.bag_name,
    ];
    for item in items.iter().filter_map(|i| i.as_ref()) {
        if !item.is_empty() {
            sqlx::query(
                "INSERT INTO item_feedback_score (user_id, item_name, score_adjustment, feedback_count) VALUES (?, ?, ?, 1) ON DUPLICATE KEY UPDATE score_adjustment = score_adjustment + ?, feedback_count = feedback_count + 1"
            )
            .bind(user_id).bind(item).bind(item_delta).bind(item_delta)
            .execute(pool).await?;
        }
    }

    // Layer 3: reason tag (중간 — ±3)
    let tag_delta = if polarity == "positive" { 3 } else { -3 };
    for reason in &req.reasons {
        // feedback_reason 로그
        sqlx::query(
            "INSERT INTO feedback_reason (feedback_id, user_id, reason_tag, polarity) VALUES (?, ?, ?, ?)"
        )
        .bind(&id).bind(user_id).bind(reason).bind(polarity)
        .execute(pool).await?;

        // user_preference_score 누적
        sqlx::query(
            "INSERT INTO user_preference_score (user_id, reason_tag, score, count) VALUES (?, ?, ?, 1) ON DUPLICATE KEY UPDATE score = score + ?, count = count + 1"
        )
        .bind(user_id).bind(reason).bind(tag_delta).bind(tag_delta)
        .execute(pool).await?;
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

pub async fn get_preference_scores(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<UserPreferenceScore>, sqlx::Error> {
    sqlx::query_as::<_, UserPreferenceScore>(
        "SELECT user_id, reason_tag, score, count FROM user_preference_score WHERE user_id = ? AND score != 0"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}
