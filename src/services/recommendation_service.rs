use anyhow::Result;
use sqlx::MySqlPool;

use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::models::recommendation::OutfitCandidate;
use crate::services::recommendation_diversity::{
    calculate_diversity_bonus, calculate_recency_penalty,
};

pub async fn rerank_candidates_with_history(
    pool: &MySqlPool,
    user_id: &str,
    mut candidates: Vec<OutfitCandidate>,
) -> Result<Vec<OutfitCandidate>> {
    for candidate in &mut candidates {
        let history = RecommendationHistoryRepo::summarize_candidate_recency(
            pool,
            user_id,
            candidate.top_id.as_deref(),
            candidate.bottom_id.as_deref(),
            candidate.outer_id.as_deref(),
            candidate.shoes_id.as_deref(),
            candidate.bag_id.as_deref(),
        )
        .await?;

        candidate.recency_penalty = calculate_recency_penalty(&history);
        candidate.diversity_bonus = calculate_diversity_bonus(&history);
        candidate.final_score =
            candidate.style_score - candidate.recency_penalty + candidate.diversity_bonus;
    }

    candidates.sort_by(|a, b| {
        b.final_score
            .cmp(&a.final_score)
            .then_with(|| b.style_score.cmp(&a.style_score))
    });

    Ok(candidates)
}

pub fn select_recommendation(candidates: &[OutfitCandidate]) -> Option<OutfitCandidate> {
    if candidates.is_empty() {
        return None;
    }

    // 상위 3개만 고려
    let top: Vec<_> = candidates.iter().take(3).cloned().collect();

    // final_score 최고 우선, 단 style_score가 너무 낮은 후보는 제외
    let filtered: Vec<_> = top.into_iter().filter(|c| c.style_score >= 70).collect();

    if filtered.is_empty() {
        return candidates.first().cloned();
    }

    filtered.into_iter().next()
}

pub async fn save_selected_recommendation(
    pool: &MySqlPool,
    user_id: &str,
    selected: &OutfitCandidate,
) -> Result<()> {
    RecommendationHistoryRepo::insert(
        pool,
        user_id,
        selected.top_id.as_deref(),
        selected.bottom_id.as_deref(),
        selected.outer_id.as_deref(),
        selected.shoes_id.as_deref(),
        selected.bag_id.as_deref(),
    )
    .await?;

    Ok(())
}
