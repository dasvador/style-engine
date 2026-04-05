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
