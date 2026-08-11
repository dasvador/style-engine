use std::collections::HashSet;

use anyhow::Result;
use sqlx::MySqlPool;

use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::models::clothing::Clothing;
use crate::models::recommendation::{OutfitCandidate, ScoringDetail};
use crate::services::recommendation_diversity::{
    calculate_diversity_bonus, calculate_dormant_bonus, calculate_recency_penalty,
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
    let top: Vec<_> = candidates.iter().take(3).cloned().collect();
    let filtered: Vec<_> = top.into_iter().filter(|c| c.style_score >= 70).collect();
    if filtered.is_empty() {
        return candidates.first().cloned();
    }
    filtered.into_iter().next()
}

// ═══════════════════════════════════════════════════════
// Today 적합도 평가 시스템
// ═══════════════════════════════════════════════════════

/// 후보의 Today 적합도 — 3가지 축으로 평가
struct TodayFitness {
    temp_suitable: bool,   // 온도 적합
    has_contrast: bool,    // 대비 충분
    has_structure: bool,   // 구조감 있음
    penalty: i32,          // 총 감점
}

impl TodayFitness {
    fn pass_count(&self) -> u8 {
        self.temp_suitable as u8 + self.has_contrast as u8 + self.has_structure as u8
    }

    /// 최소 2/3 조건 통과해야 Today 후보 자격
    fn is_qualified(&self) -> bool {
        self.pass_count() >= 2
    }
}

fn find_clothing<'a>(id: &Option<String>, clothes: &'a [Clothing]) -> Option<&'a Clothing> {
    id.as_ref().and_then(|id| clothes.iter().find(|c| c.id == *id))
}

fn evaluate_today_fitness(
    c: &OutfitCandidate,
    temperature: f64,
    clothes: &[Clothing],
) -> TodayFitness {
    let top = find_clothing(&c.top_id, clothes);
    let bottom = find_clothing(&c.bottom_id, clothes);
    let outer = find_clothing(&c.outer_id, clothes);

    let mut penalty = 0;

    // ─── 1. 온도 적합성 ───
    let is_light_top = top
        .is_some_and(|t| t.weight.as_deref() == Some("가벼움") || t.thickness == "thin");
    let has_outer = outer.is_some();

    let temp_suitable = if temperature <= 13.0 {
        // 13도 이하: 반팔 단독 reject
        if is_light_top && !has_outer {
            penalty += 20;
            false
        } else {
            true
        }
    } else if temperature <= 18.0 {
        // 13~18도: 반팔 허용하되 아우터/긴팔 선호
        if is_light_top && !has_outer {
            penalty += 8;
            false // soft fail — 감점은 있지만 다른 조건으로 보완 가능
        } else {
            true
        }
    } else {
        true // 18도 이상: 제한 없음
    };

    // ─── 2. 대비 충분성 ───
    let top_tone = top.and_then(|t| t.tone.as_deref());
    let bottom_tone = bottom.and_then(|t| t.tone.as_deref());
    let top_temp = top.and_then(|t| t.color_temperature.as_deref());
    let bottom_temp = bottom.and_then(|t| t.color_temperature.as_deref());

    let has_contrast = match (top_tone, bottom_tone) {
        (Some(tt), Some(bt)) => {
            if tt == bt {
                // 같은 밝기 → 색온도라도 달라야 함
                let same_color_temp = top_temp.is_some() && top_temp == bottom_temp;
                if same_color_temp {
                    // 완전 동일 톤+색온도: 강한 감점
                    penalty += 12;
                    false
                } else if tt == "밝음" {
                    // 둘 다 밝음 (색온도는 다름): 약한 감점
                    let outer_helps = outer
                        .and_then(|o| o.tone.as_deref())
                        .is_some_and(|t| t == "어두움" || t == "중간");
                    if outer_helps {
                        true
                    } else {
                        penalty += 8;
                        false
                    }
                } else {
                    true // 둘 다 어두움/중간이지만 색온도 다름 → OK
                }
            } else {
                true // 밝기가 다름 → 대비 있음
            }
        }
        _ => true, // 데이터 없음 → 패스
    };

    // ─── 3. 구조감 ───
    let bottom_structured = bottom.is_some_and(|b| {
        let role = b.role.as_deref().unwrap_or("");
        let style = b.style.as_deref().unwrap_or("");
        let weight = b.weight.as_deref().unwrap_or("");
        role == "구조템" || style == "포멀" || weight == "무거움"
    });
    let outer_structured = outer.is_some_and(|o| {
        let role = o.role.as_deref().unwrap_or("");
        role == "구조템" || role == "약한포인트" || role == "포인트"
    });
    let top_is_structure = top.is_some_and(|t| {
        t.role.as_deref() == Some("구조템")
    });

    let has_structure = bottom_structured || outer_structured || top_is_structure;
    if !has_structure {
        penalty += 7;
    }

    // ─── 4. 역할 밸런스 (포인트 과다 / 올 베이직) ───
    let roles: Vec<&str> = [top, bottom, outer]
        .iter()
        .filter_map(|item| item.and_then(|i| i.role.as_deref()))
        .collect();

    let accent_count = roles
        .iter()
        .filter(|r| **r == "포인트" || **r == "약한포인트")
        .count();
    if accent_count >= 2 {
        penalty += 5;
    }

    TodayFitness {
        temp_suitable,
        has_contrast,
        has_structure,
        penalty,
    }
}

// ═══════════════════════════════════════════════════════
// Mode-specific sequential selection
// ═══════════════════════════════════════════════════════

pub struct ModeSelectionResult {
    pub candidate: OutfitCandidate,
    pub scoring: ScoringDetail,
}

/// Step 1: 오늘의 추천 — 적합도 게이트 + 최고 점수
pub fn select_todays_pick(
    candidates: &[OutfitCandidate],
    temperature: f64,
    clothes: &[Clothing],
) -> Option<ModeSelectionResult> {
    // 각 후보에 적합도 평가
    let scored: Vec<(&OutfitCandidate, TodayFitness, i32)> = candidates
        .iter()
        .map(|c| {
            let fitness = evaluate_today_fitness(c, temperature, clothes);
            let adjusted = c.final_score - fitness.penalty;
            (c, fitness, adjusted)
        })
        .collect();

    // 1차: style >= 70 + 2/3 조건 통과 후보
    let pick = scored
        .iter()
        .filter(|(c, f, _)| c.style_score >= 70 && f.is_qualified())
        .max_by_key(|(_, _, adj)| *adj)
        // 2차: style >= 70 + 감점 적용
        .or_else(|| {
            scored
                .iter()
                .filter(|(c, _, _)| c.style_score >= 70)
                .max_by_key(|(_, _, adj)| *adj)
        })
        // 3차: 전체에서 최고점
        .or_else(|| scored.iter().max_by_key(|(_, _, adj)| *adj));

    pick.map(|(c, fitness, adjusted)| ModeSelectionResult {
        scoring: ScoringDetail {
            style_score: c.style_score,
            recency_penalty: c.recency_penalty + fitness.penalty,
            diversity_bonus: c.diversity_bonus,
            dormant_bonus: 0,
            final_score: *adjusted,
        },
        candidate: (*c).clone(),
    })
}

/// Step 2: 다른 조합 — Today와 상의+하의가 다른 후보 중 최고 점수
pub fn select_variation(
    candidates: &[OutfitCandidate],
    todays_pick: &OutfitCandidate,
) -> Option<ModeSelectionResult> {
    // 1차: 상의+하의 모두 다른 후보
    let different_pair: Vec<_> = candidates
        .iter()
        .filter(|c| !has_same_top_bottom(c, todays_pick))
        .collect();

    if let Some(best) = pick_best_variation(&different_pair) {
        return Some(best);
    }

    // 2차: 상의 또는 하의 중 하나라도 다른 후보
    let different_one: Vec<_> = candidates
        .iter()
        .filter(|c| !is_same_outfit(c, todays_pick))
        .collect();

    if let Some(best) = pick_best_variation(&different_one) {
        return Some(best);
    }

    // 3차 폴백: 두 번째 후보
    candidates
        .iter()
        .skip(1)
        .next()
        .map(|c| ModeSelectionResult {
            scoring: build_variation_scoring(c),
            candidate: c.clone(),
        })
}

/// Step 3: 안 입은 옷 — 휴면 아이템을 포함하면서 Today와 top+bottom이 다르고
/// 품질 기준을 통과하는 후보가 있을 때만 추천. 없으면 None (옵셔널 카드)
pub fn select_dormant(
    candidates: &[OutfitCandidate],
    dormant_ids: &HashSet<String>,
    todays_pick: &OutfitCandidate,
) -> Option<ModeSelectionResult> {
    let pool: Vec<_> = candidates
        .iter()
        .filter(|c| contains_dormant_item(c, dormant_ids))
        .filter(|c| !has_same_top_bottom(c, todays_pick))
        .collect();

    pick_best_dormant(&pool, dormant_ids)
}

// ─── Helper functions ───

pub fn has_same_top_bottom(a: &OutfitCandidate, b: &OutfitCandidate) -> bool {
    a.top_id.is_some()
        && a.top_id == b.top_id
        && a.bottom_id.is_some()
        && a.bottom_id == b.bottom_id
}

pub fn is_same_outfit(a: &OutfitCandidate, b: &OutfitCandidate) -> bool {
    a.top_id == b.top_id && a.bottom_id == b.bottom_id && a.outer_id == b.outer_id
}

pub fn contains_dormant_item(candidate: &OutfitCandidate, dormant_ids: &HashSet<String>) -> bool {
    [
        &candidate.top_id,
        &candidate.bottom_id,
        &candidate.outer_id,
        &candidate.shoes_id,
        &candidate.bag_id,
    ]
    .iter()
    .any(|id| id.as_ref().is_some_and(|id| dormant_ids.contains(id)))
}

fn pick_best_variation(pool: &[&OutfitCandidate]) -> Option<ModeSelectionResult> {
    pool.iter()
        .filter(|c| c.style_score >= 68)
        .max_by_key(|c| c.style_score + c.diversity_bonus * 3 - c.recency_penalty)
        .or_else(|| pool.first())
        .map(|c| ModeSelectionResult {
            scoring: build_variation_scoring(c),
            candidate: (*c).clone(),
        })
}

fn pick_best_dormant(
    pool: &[&OutfitCandidate],
    dormant_ids: &HashSet<String>,
) -> Option<ModeSelectionResult> {
    pool.iter()
        .filter(|c| c.style_score >= 70)
        .max_by_key(|c| {
            let dormant = calculate_dormant_bonus(c, dormant_ids);
            c.style_score + dormant
        })
        .map(|c| {
            let dormant = calculate_dormant_bonus(c, dormant_ids);
            ModeSelectionResult {
                scoring: ScoringDetail {
                    style_score: c.style_score,
                    recency_penalty: c.recency_penalty,
                    diversity_bonus: c.diversity_bonus,
                    dormant_bonus: dormant,
                    final_score: c.style_score - c.recency_penalty + c.diversity_bonus + dormant,
                },
                candidate: (*c).clone(),
            }
        })
}

fn build_variation_scoring(c: &OutfitCandidate) -> ScoringDetail {
    let mode_score = c.style_score - (c.recency_penalty as f64 * 1.8).floor() as i32
        + (c.diversity_bonus as f64 * 2.5).floor() as i32;
    ScoringDetail {
        style_score: c.style_score,
        recency_penalty: c.recency_penalty,
        diversity_bonus: c.diversity_bonus,
        dormant_bonus: 0,
        final_score: mode_score,
    }
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
