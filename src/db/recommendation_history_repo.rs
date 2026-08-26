use std::collections::HashSet;

use sqlx::MySqlPool;

use crate::models::recommendation_history::{
    OutfitHistorySummary, OutfitRecommendationHistory, new_history_id,
};

pub struct RecommendationHistoryRepo;

impl RecommendationHistoryRepo {
    pub async fn insert(
        pool: &MySqlPool,
        user_id: &str,
        top_id: Option<&str>,
        bottom_id: Option<&str>,
        outer_id: Option<&str>,
        shoes_id: Option<&str>,
        bag_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id = new_history_id();

        sqlx::query(
            r#"
            INSERT INTO outfit_recommendation_history
            (id, user_id, top_id, bottom_id, outer_id, shoes_id, bag_id)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(user_id)
        .bind(top_id)
        .bind(bottom_id)
        .bind(outer_id)
        .bind(shoes_id)
        .bind(bag_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn find_recent_by_user(
        pool: &MySqlPool,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<OutfitRecommendationHistory>, sqlx::Error> {
        let rows = sqlx::query_as::<_, OutfitRecommendationHistory>(
            r#"
            SELECT id, user_id, top_id, bottom_id, outer_id, shoes_id, bag_id, recommended_at
            FROM outfit_recommendation_history
            WHERE user_id = ?
            ORDER BY recommended_at DESC
            LIMIT ?
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn summarize_candidate_recency(
        pool: &MySqlPool,
        user_id: &str,
        top_id: Option<&str>,
        bottom_id: Option<&str>,
        outer_id: Option<&str>,
        shoes_id: Option<&str>,
        bag_id: Option<&str>,
    ) -> Result<OutfitHistorySummary, sqlx::Error> {
        let recent = Self::find_recent_by_user(pool, user_id, 50).await?;
        let mut summary = OutfitHistorySummary::default();

        for row in &recent {
            let days_ago = {
                let now = chrono::Local::now().naive_local();
                (now - row.recommended_at).num_days()
            };

            let same_item = [
                (top_id, row.top_id.as_deref()),
                (bottom_id, row.bottom_id.as_deref()),
                (outer_id, row.outer_id.as_deref()),
                (shoes_id, row.shoes_id.as_deref()),
                (bag_id, row.bag_id.as_deref()),
            ]
            .iter()
            .any(|(a, b)| a.is_some() && *a == *b);

            if same_item && days_ago <= 1 {
                summary.item_used_yesterday = true;
            }

            if same_item && days_ago <= 3 {
                summary.item_used_within_3_days = true;
            }

            if days_ago <= 7
                && top_id == row.top_id.as_deref()
                && bottom_id == row.bottom_id.as_deref()
            {
                summary.same_top_bottom_within_7_days = true;
            }

            if days_ago <= 14
                && top_id == row.top_id.as_deref()
                && bottom_id == row.bottom_id.as_deref()
                && outer_id == row.outer_id.as_deref()
                && shoes_id == row.shoes_id.as_deref()
                && bag_id == row.bag_id.as_deref()
            {
                summary.same_full_outfit_within_14_days = true;
            }

            if days_ago <= 7 && same_item {
                summary.recent_usage_count_7d += 1;
            }
        }

        Ok(summary)
    }

    /// 14일 이상 추천되지 않은 휴면 아이템 ID 집합
    pub async fn find_dormant_item_ids(
        pool: &MySqlPool,
        user_id: &str,
        all_clothing_ids: &[String],
    ) -> Result<HashSet<String>, sqlx::Error> {
        let recent = Self::find_recent_by_user(pool, user_id, 100).await?;
        let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(14);

        let mut recently_used = HashSet::new();
        for row in &recent {
            if row.recommended_at >= cutoff {
                for id in [
                    &row.top_id,
                    &row.bottom_id,
                    &row.outer_id,
                    &row.shoes_id,
                    &row.bag_id,
                ]
                .into_iter()
                .flatten()
                {
                    recently_used.insert(id.clone());
                }
            }
        }

        let dormant = all_clothing_ids
            .iter()
            .filter(|id| !recently_used.contains(*id))
            .cloned()
            .collect();

        Ok(dormant)
    }
}
