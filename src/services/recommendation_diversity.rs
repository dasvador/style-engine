use std::collections::HashSet;

use crate::models::clothing::Clothing;
use crate::models::recommendation::OutfitCandidate;
use crate::models::recommendation_history::OutfitHistorySummary;

pub fn calculate_recency_penalty(history: &OutfitHistorySummary) -> i32 {
    let mut penalty = 0;

    if history.item_used_yesterday {
        penalty += 15;
    }

    if history.item_used_within_3_days {
        penalty += 8;
    }

    if history.same_top_bottom_within_7_days {
        penalty += 20;
    }

    if history.same_full_outfit_within_14_days {
        penalty += 25;
    }

    penalty += (history.recent_usage_count_7d / 2).min(8);

    penalty
}

pub fn calculate_diversity_bonus(history: &OutfitHistorySummary) -> i32 {
    if !history.item_used_within_3_days && !history.same_top_bottom_within_7_days {
        6
    } else if !history.item_used_yesterday {
        3
    } else {
        0
    }
}

/// 후보 내 휴면 아이템(14일+ 미사용) 개수에 따른 보너스
pub fn calculate_dormant_bonus(candidate: &OutfitCandidate, dormant_ids: &HashSet<String>) -> i32 {
    let count = candidate_slot_ids(candidate)
        .filter(|id| dormant_ids.contains(id.as_str()))
        .count();

    match count {
        0 => 0,
        1 => 12,
        2 => 20,
        _ => 25,
    }
}

/// 후보 내 휴면 아이템 이름 목록
pub fn find_dormant_items_in_candidate(
    candidate: &OutfitCandidate,
    dormant_ids: &HashSet<String>,
    clothes: &[Clothing],
) -> Vec<String> {
    candidate_slot_ids(candidate)
        .filter(|id| dormant_ids.contains(id.as_str()))
        .filter_map(|id| clothes.iter().find(|c| c.id == *id))
        .map(|c| c.name.clone())
        .collect()
}

fn candidate_slot_ids(c: &OutfitCandidate) -> impl Iterator<Item = &String> {
    [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
        .into_iter()
        .flatten()
}
