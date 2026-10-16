use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::feedback::{FeedbackRequest, ItemFeedbackScore, UserPreferenceScore};

pub async fn insert_feedback(
    pool: &MySqlPool,
    user_id: &str,
    req: &FeedbackRequest,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let reason_str = if req.reasons.is_empty() {
        None
    } else {
        Some(req.reasons.join(","))
    };

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

/// 같은 착장에 남긴 가장 최근의 피드백 하나를 되돌린다. 되돌린 것이 있으면 true.
///
/// 홈의 '마음에 들어요' / '오늘 입을래요' 를 잘못 눌렀을 때 취소하는 데 쓴다. 화면은
/// 피드백 id 를 들고 있지 않으므로(새로고침하면 사라진다) 아이템 이름과 종류로 찾는다.
/// `insert_feedback` 이 더한 만큼 아이템 점수와 이유 태그 점수를 빼고 행을 지운다 —
/// 기록만 남기고 점수를 그대로 두면 취소한 좋아요가 다음 추천에 계속 반영된다.
pub async fn undo_latest_feedback(
    pool: &MySqlPool,
    user_id: &str,
    feedback_type: &str,
    names: [Option<&str>; 5],
) -> Result<bool, sqlx::Error> {
    let [inner, outer, bottom, shoes, bag] = names;
    let mut tx = pool.begin().await?;
    let found: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM outfit_feedback \
         WHERE user_id = ? AND feedback_type = ? AND inner_name <=> ? AND outer_name <=> ? \
           AND bottom_name <=> ? AND shoes_name <=> ? AND bag_name <=> ? \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(feedback_type)
    .bind(inner)
    .bind(outer)
    .bind(bottom)
    .bind(shoes)
    .bind(bag)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((id,)) = found else {
        return Ok(false);
    };

    let item_delta = match feedback_type {
        "like" | "worn" | "saved" => 1,
        "dislike" => -1,
        _ => 0,
    };
    for item in [inner, outer, bottom, shoes, bag].into_iter().flatten() {
        if item.is_empty() {
            continue;
        }
        sqlx::query(
            "UPDATE item_feedback_score \
             SET score_adjustment = score_adjustment - ?, \
                 feedback_count = GREATEST(feedback_count - 1, 0) \
             WHERE user_id = ? AND item_name = ?",
        )
        .bind(item_delta)
        .bind(user_id)
        .bind(item)
        .execute(&mut *tx)
        .await?;
    }

    let reasons: Vec<(String, String)> =
        sqlx::query_as("SELECT reason_tag, polarity FROM feedback_reason WHERE feedback_id = ?")
            .bind(&id)
            .fetch_all(&mut *tx)
            .await?;
    for (tag, polarity) in &reasons {
        let tag_delta = if polarity == "positive" { 3 } else { -3 };
        sqlx::query(
            "UPDATE user_preference_score \
             SET score = score - ?, count = GREATEST(count - 1, 0) \
             WHERE user_id = ? AND reason_tag = ?",
        )
        .bind(tag_delta)
        .bind(user_id)
        .bind(tag)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM feedback_reason WHERE feedback_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM outfit_feedback WHERE id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}
