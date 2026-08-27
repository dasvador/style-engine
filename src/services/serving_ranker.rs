//! Serving Ranker (v2 S4) — context-aware serving gate + ranking.
//!
//! style_score(S3)는 미적 판단만 담당하고, 이 모듈은 운영 판단을 담당한다:
//!   - 온도/상황에 맞는지 (TodayFitLevel gate)
//!   - 상황별 accessory 민감도 보정 (serving_adjustment)
//!
//! style_score 본체는 건드리지 않는다. serving_adjustment는 별도 합산.
//! baseline에 영향 없음 — shadow experiment 경로에서만 사용.

use crate::models::outfit::{OutfitContext, SlotKind};
use crate::models::style_vocab::{Style, Thickness, Weight};
use crate::services::style_engine_v2::TodayFitLevel;

/// Today 적합도 판정.
///
/// 순서:
///   1. situation-aware gate (출근/비즈니스/데이트)
///   2. temperature gate (온도 + 아우터 유무)
///   3. 위 어디에도 걸리지 않으면 Pass
pub fn compute_today_fit(ctx: &OutfitContext, temperature: f64) -> TodayFitLevel {
    let situation = ctx.situation.as_deref();
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);

    let has_sport_shoes = ctx
        .slots
        .iter()
        .filter(|s| s.slot == SlotKind::Shoes)
        .any(|s| s.clothing.style == Some(Style::Sport));

    let has_sweat_top = top.is_some_and(|t| t.texture_worlds.iter().any(|w| w == "sweat"));
    let has_sweat_bottom = ctx
        .slots
        .iter()
        .filter(|s| s.slot == SlotKind::Bottom)
        .any(|s| s.texture_worlds.iter().any(|w| w == "sweat"));

    let formality_avg = compute_formality_avg(ctx);

    // ─── 1. Situation-aware gate ───
    if let Some(sit) = situation {
        match sit {
            "출근" | "비즈니스" => {
                // 스웻셋업 → Fail
                if has_sweat_top && has_sweat_bottom {
                    return TodayFitLevel::Fail;
                }
                // 스포츠 슈즈 → Fail
                if has_sport_shoes {
                    return TodayFitLevel::Fail;
                }
                // 격식 전체적으로 낮음 → Fail or Borderline
                if formality_avg < 2.0 {
                    return TodayFitLevel::Fail;
                }
                if formality_avg < 3.0 {
                    return TodayFitLevel::Borderline;
                }
            }
            "데이트"
                // 스포츠 슈즈 → 격식 수준에 따라 Fail or Borderline
                // TODO(tuning): HC024 유형 — 사용자 기대는 Fail이지만 현재 formality_avg < 2.5라
                // Borderline 판정. 임계치를 낮추거나 데이트+스포츠슈즈를 일괄 Fail로 변경 검토.
                if has_sport_shoes => {
                    if formality_avg >= 2.5 {
                        return TodayFitLevel::Fail;
                    }
                    return TodayFitLevel::Borderline;
                }
            _ => {} // 캐주얼/일상/주말 → 관대
        }
    }

    // ─── 2. Temperature gate ───
    let is_light_top = top.is_some_and(|t| {
        t.clothing.weight == Some(Weight::Light) || t.clothing.thickness == Thickness::Thin
    });

    if temperature <= 13.0 && is_light_top && !has_outer {
        return TodayFitLevel::Fail;
    }
    if temperature <= 18.0 && is_light_top && !has_outer {
        return TodayFitLevel::Borderline;
    }

    TodayFitLevel::Pass
}

/// Serving adjustment — situation-aware 보정. style_score에 합산하지 않고 별도.
/// 반환: (adjustment, reason)
pub fn compute_serving_adjustment(ctx: &OutfitContext) -> (i32, String) {
    let situation = ctx.situation.as_deref();
    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    let bottom = ctx.slots.iter().find(|s| s.slot == SlotKind::Bottom);
    let shoes = ctx.slots.iter().find(|s| s.slot == SlotKind::Shoes);
    let bag = ctx.slots.iter().find(|s| s.slot == SlotKind::Bag);

    let clothing_formality = {
        let levels: Vec<f32> = [top, bottom]
            .iter()
            .filter_map(|x| x.and_then(|s| s.clothing.formality_level.map(|l| l as f32)))
            .collect();
        if levels.is_empty() {
            2.0
        } else {
            levels.iter().sum::<f32>() / levels.len() as f32
        }
    };

    // situation에 따라 accessory gap 가중치 결정
    let weight = match situation {
        Some("출근" | "비즈니스") => 2.0,
        Some("데이트") => 1.5,
        _ => 1.0,
    };

    let mut adj = 0i32;
    let mut reasons: Vec<String> = Vec::new();

    // 신발 격식 gap — 비대칭: under-formal 강하게, over-formal 약하게.
    // under: 출근에 러닝화/스니커 → 큰 감점
    // over: 캐주얼에 더비/로퍼 → 소폭 감점
    if let Some(shoe) = shoes {
        let shoe_f = shoe.clothing.formality_level.unwrap_or(2) as f32;
        let raw_gap = shoe_f - clothing_formality; // 양수=over, 음수=under
        let is_under = raw_gap < 0.0;
        let abs_gap = raw_gap.abs();

        if abs_gap >= 1.0 {
            let direction_mult = if is_under { 1.8 } else { 0.6 };
            let pen = if abs_gap >= 2.0 {
                (abs_gap * weight * 3.0 * direction_mult) as i32
            } else {
                (abs_gap * weight * 1.5 * direction_mult) as i32
            };
            if pen > 0 {
                adj -= pen;
                let label = if is_under { "under" } else { "over" };
                reasons.push(format!("shoe_{label}-{pen}"));
            }
        }
    }

    // 가방 격식 gap (소폭)
    if let Some(b) = bag {
        let bag_f = b.clothing.formality_level.unwrap_or(2) as f32;
        let gap = (bag_f - clothing_formality).abs();
        if gap >= 2.0 {
            let pen = (gap * weight) as i32;
            adj -= pen;
            reasons.push(format!("bag_gap-{pen}"));
        }
    }

    let reason = if reasons.is_empty() {
        "none".to_string()
    } else {
        reasons.join(",")
    };
    (adj, reason)
}

/// 후보 목록을 serving 순서로 정렬.
///
/// 순서:
///   1차: hard_pass (true 우선)
///   2차: today_fit != Fail (Pass/Borderline 우선)
///   3차: style_score + serving_adjustment 내림차순
///   4차: recency_penalty 오름차순
///   5차: diversity_bonus 내림차순
///   6차: ai_candidate_index 오름차순
pub fn serving_sort_key(
    hard_pass: bool,
    today_fit: TodayFitLevel,
    serving_score: i32,
    recency_penalty: i32,
    diversity_bonus: i32,
    index: usize,
) -> impl Ord {
    let tier = match (hard_pass, today_fit) {
        (true, TodayFitLevel::Pass) => 0,
        (true, TodayFitLevel::Borderline) => 1,
        (true, TodayFitLevel::Fail) => 2,
        (false, _) => 3,
    };
    // negate for descending where needed
    (
        tier,
        -serving_score,
        recency_penalty,
        -diversity_bonus,
        index,
    )
}

fn compute_formality_avg(ctx: &OutfitContext) -> f32 {
    let levels: Vec<f32> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.formality_level.map(|l| l as f32))
        .collect();
    if levels.is_empty() {
        2.0
    } else {
        levels.iter().sum::<f32>() / levels.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::clothing::Clothing;
    use crate::models::outfit::OutfitSlot;
    use crate::models::style_vocab::Tone;
    use chrono::NaiveDateTime;

    fn ts() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-08-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// 온도 게이트가 보는 두 속성만 지정하고 나머지는 중립값으로 채운다.
    fn top(weight: Weight, thickness: Thickness) -> OutfitSlot {
        OutfitSlot {
            slot: SlotKind::Top,
            clothing: Clothing {
                id: "t".into(),
                name: "테스트 상의".into(),
                category: "상의".into(),
                gender: None,
                style_mood: None,
                color: None,
                thickness,
                image_url: None,
                tone: Some(Tone::Mid),
                saturation: None,
                style: None,
                weight: Some(weight),
                role: None,
                color_temperature: None,
                versatility: None,
                statement_level: None,
                formality_level: Some(2),
                visual_weight: None,
                texture_depth: None,
                visual_weight_v2: None,
                texture_depth_v2: None,
                grounding_score: None,
                shadow_tone: None,
                silhouette_volume: None,
                material_primary: None,
                sub_category: None,
                floating_score: None,
                strong_style_score: None,
                texture_keywords: None,
                created_at: ts(),
                updated_at: ts(),
            },
            seasons: Vec::new(),
            texture_worlds: Vec::new(),
        }
    }

    fn ctx(slot: OutfitSlot) -> OutfitContext {
        OutfitContext {
            slots: vec![slot],
            situation: None,
        }
    }

    /// 온도 게이트는 `weight == 가벼움` 또는 `thickness == thin` 중 하나만 참이어도 걸린다.
    /// 이 테스트가 필요한 이유: 케이스 카탈로그에는 시각적 무게가 '중간'이면서 원단만 얇은
    /// 상의를 저온에 세우는 케이스가 없어서, eval 로는 thickness 분기가 한 번도 실행되지 않는다.
    /// 실제로 프로덕션에서는 이 분기가 어휘 불일치로 39건에 대해 죽어 있었다.
    #[test]
    fn thin_fabric_alone_triggers_the_cold_gate() {
        // 시각적 무게는 중간 — weight 조건으로는 걸리지 않는다.
        let mid_thin = ctx(top(Weight::Mid, Thickness::Thin));
        assert_eq!(compute_today_fit(&mid_thin, 10.0), TodayFitLevel::Fail);
        assert_eq!(
            compute_today_fit(&mid_thin, 16.0),
            TodayFitLevel::Borderline
        );
        assert_eq!(compute_today_fit(&mid_thin, 22.0), TodayFitLevel::Pass);
    }

    #[test]
    fn light_weight_alone_triggers_the_cold_gate() {
        let light_medium = ctx(top(Weight::Light, Thickness::Medium));
        assert_eq!(compute_today_fit(&light_medium, 10.0), TodayFitLevel::Fail);
        assert_eq!(
            compute_today_fit(&light_medium, 16.0),
            TodayFitLevel::Borderline
        );
    }

    /// 두 조건 모두 거짓이면 저온이어도 통과해야 한다.
    #[test]
    fn mid_weight_medium_fabric_passes_in_the_cold() {
        let mid_medium = ctx(top(Weight::Mid, Thickness::Medium));
        assert_eq!(compute_today_fit(&mid_medium, 10.0), TodayFitLevel::Pass);
    }

    /// 아우터가 있으면 얇은 상의여도 게이트가 걸리지 않는다.
    #[test]
    fn an_outer_layer_lifts_the_gate() {
        let mut c = ctx(top(Weight::Light, Thickness::Thin));
        let mut outer = top(Weight::Heavy, Thickness::Thick);
        outer.slot = SlotKind::Outer;
        c.slots.push(outer);
        assert_eq!(compute_today_fit(&c, 10.0), TodayFitLevel::Pass);
    }
}
