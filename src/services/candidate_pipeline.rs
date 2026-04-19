//! Candidate Pipeline (v2) — experiment 경로 전용 후보 전처리.

use sqlx::MySqlPool;

use crate::db::clothing_repo;
use crate::models::clothing::Clothing;
use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use crate::models::recommendation::OutfitCandidate;
use crate::services::style_engine_v2::{self, OutfitEvaluation};
use crate::services::serving_ranker;

/// 후보의 아이템 id들로부터 OutfitContext를 재구성.
pub async fn rebuild_context(
    candidate: &OutfitCandidate,
    clothes: &[Clothing],
    db: &MySqlPool,
    occasion: Option<&str>,
) -> Option<OutfitContext> {
    let slot_ids: [(Option<&str>, SlotKind); 5] = [
        (candidate.top_id.as_deref(), SlotKind::Top),
        (candidate.bottom_id.as_deref(), SlotKind::Bottom),
        (candidate.outer_id.as_deref(), SlotKind::Outer),
        (candidate.shoes_id.as_deref(), SlotKind::Shoes),
        (candidate.bag_id.as_deref(), SlotKind::Bag),
    ];

    let mut slots = Vec::new();
    for (maybe_id, kind) in slot_ids {
        let Some(id) = maybe_id else { continue };
        let Some(clothing) = clothes.iter().find(|c| c.id == id) else { continue };
        let seasons = clothing_repo::get_seasons(db, id).await.unwrap_or_default();
        let texture_worlds = clothing_repo::get_texture_worlds(db, id)
            .await
            .unwrap_or_default();
        slots.push(OutfitSlot {
            slot: kind,
            clothing: clothing.clone(),
            seasons,
            texture_worlds,
        });
    }

    if slots.is_empty() {
        return None;
    }
    Some(OutfitContext {
        slots,
        situation: occasion.map(|s| s.to_string()),
    })
}

/// v2 평가 — hard filter + subscore(S3) + today_fit(S4).
pub fn evaluate_v2(
    ctx: &OutfitContext,
    current_season: Option<&str>,
    temperature: f64,
) -> OutfitEvaluation {
    let hard = style_engine_v2::run_hard_filter(ctx, current_season);
    let sub = style_engine_v2::compute_subscores(ctx, current_season);
    let style_score = style_engine_v2::compute_style_score(&sub);
    let today_fit = serving_ranker::compute_today_fit(ctx, temperature);
    OutfitEvaluation {
        hard,
        sub,
        style_score,
        today_fit,
    }
}
