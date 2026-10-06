//! Serving Ranker (v2 S4) — context-aware serving gate + ranking.
//!
//! style_score(S3)는 미적 판단만 담당하고, 이 모듈은 운영 판단을 담당한다:
//!   - 온도/상황에 맞는지 (TodayFitLevel gate)
//!   - 상황별 accessory 민감도 보정 (serving_adjustment)
//!
//! style_score 본체는 건드리지 않는다. serving_adjustment는 별도 합산.
//! baseline에 영향 없음 — shadow experiment 경로에서만 사용.

use crate::models::clothing::Clothing;
use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use crate::models::style_vocab::{Style, Thickness, Weight};
use crate::services::style_engine_v2::TodayFitLevel;

// ─── 온도 게이트 임계값 ───
// 케이스 카탈로그의 라벨에 맞춰 정한 값이고, 바꾸면 eval 스코어카드가 즉시 반응한다.

/// 요구 보온 점수에 이만큼 이상 모자라면 실패.
const COLD_FAIL_DEFICIT: i32 = 3;
/// 이만큼 모자라면 경계. 1점 차이는 통과로 둔다 — 20도에 반팔 한 장은 정상이다.
const COLD_BORDERLINE_DEFICIT: i32 = 2;
/// 이 온도 이상에서 상하의가 모두 두꺼우면 실패.
const HEAT_FAIL_C: f64 = 26.0;
/// 아우터 가산점 — 같은 두께라도 겉옷이 체감에 더 기여한다.
const OUTER_BONUS: i32 = 2;

/// accessory 격식 gap 패널티가 이 값 이하이면 Pass 를 Borderline 으로 내린다.
///
/// 라벨은 "신발만 러닝화로 바꿨다", "가방만 백팩으로 바꿨다" 같은 케이스를 경계로 보는데,
/// 그 신호는 이미 `compute_serving_adjustment` 가 격식 gap 으로 계산하고 있었다.
/// today_fit 이 그 값을 보지 않아서 전부 Pass 로 나가던 것을 연결한다.
/// 값은 현재 Pass 판정 케이스들의 패널티 분포에서 정했다 — 이보다 완만하게 잡으면
/// 정상 Pass 가 깎이고, 더 엄격하게 잡으면 경계 케이스를 놓친다.
const ACCESSORY_PENALTY_BORDERLINE: i32 = -4;

/// Today 적합도 판정.
///
/// 순서:
///   1. situation-aware gate (출근/비즈니스/데이트)
///   2. temperature gate (기온이 요구하는 보온 점수 vs 토르소 레이어 합)
///   3. 위 어디에도 걸리지 않으면 Pass
pub fn compute_today_fit(ctx: &OutfitContext, temperature: f64) -> TodayFitLevel {
    let situation = ctx.situation.as_deref();
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
            // 데이트에 러닝화는 나머지 착장의 격식과 무관하게 실패로 본다.
            // 이전에는 formality_avg >= 2.5 일 때만 Fail 이었는데, 캐주얼한 착장일수록
            // 평균 격식이 낮아져 오히려 통과하는 역전이 있었다.
            "데이트" if has_sport_shoes => {
                return TodayFitLevel::Fail;
            }
            _ => {} // 캐주얼/일상/주말 → 관대
        }
    }

    // ─── 2. Temperature gate ───
    //
    // 상의 한 장만 보고 판정하지 않고 토르소 레이어(상의 + 아우터)의 보온 점수를
    // 합쳐 기온이 요구하는 값과 비교한다. 이전에는 "얇거나 가벼운 상의 + 아우터 없음"
    // 이라는 한 가지 모양만 봤고, 그래서 두 방향이 모두 틀렸다:
    //   - 얇은 블라우스 + 얇은 윈드브레이커는 아우터가 있다는 이유만으로 통과했고,
    //   - 미디엄 니트 단독은 아우터가 없어도 영하까지 통과했다.
    // 보온은 한 장의 속성이 아니라 겹쳐 입은 결과이므로 합으로 본다.
    let heavy_layer = |slot: SlotKind| {
        ctx.slots.iter().filter(|s| s.slot == slot).any(|s| {
            s.clothing.weight == Some(Weight::Heavy) || s.clothing.thickness == Thickness::Thick
        })
    };

    // 더위 쪽은 추위보다 먼저 본다 — 28도에 두꺼운 상하의는 보온 과잉이 아니라 실패다.
    if temperature >= HEAT_FAIL_C && heavy_layer(SlotKind::Top) && heavy_layer(SlotKind::Bottom) {
        return TodayFitLevel::Fail;
    }

    // 상의가 없는 후보(슬롯 해석 실패 등)는 보온을 따질 대상이 아니다.
    if let Some(top) = top {
        let warmth = torso_warmth(top, ctx);
        let required = required_torso_warmth(temperature);
        let deficit = required - warmth;

        if deficit >= COLD_FAIL_DEFICIT {
            return TodayFitLevel::Fail;
        }
        if deficit >= COLD_BORDERLINE_DEFICIT {
            return TodayFitLevel::Borderline;
        }
        // 보온 과잉은 여기서 판정하지 않는다. 케이스 카탈로그의 라벨이 이 축에서
        // 갈리지 않는다 — 20도에 무거운 울자켓(TG008)은 경계인데 같은 20도에 파카를
        // 걸친 조합(SG014)은 통과로 달려 있고, 두 조합의 보온 점수는 같다. 과잉 분기를
        // 넣으면 한쪽을 맞히는 대가로 다른 쪽을 틀린다. 진짜 더위는 위의 HEAT_FAIL_C
        // 규칙이 잡는다.
    }
    // ─── 3. Accessory 격식 gap ───
    // 온도·상황 게이트를 다 통과했어도, 신발/가방의 격식이 착장과 크게 어긋나면
    // "오늘 그대로 입기엔 애매한" 상태로 본다.
    let (accessory_adj, _) = compute_serving_adjustment(ctx);
    if accessory_adj <= ACCESSORY_PENALTY_BORDERLINE {
        return TodayFitLevel::Borderline;
    }

    TodayFitLevel::Pass
}

/// 토르소 레이어 하나의 보온 점수.
///
/// 원단 두께를 뼈대로 하고 시각적 무게로 보정한다. 두께가 비어 있으면 DB 기본값인
/// `medium` 이 들어오므로 별도 분기는 두지 않는다.
fn layer_warmth(c: &Clothing) -> i32 {
    let base = match c.thickness {
        Thickness::Thin => 2,
        Thickness::Medium => 4,
        Thickness::Thick => 7,
    };
    let adj = match c.weight {
        Some(Weight::Light) => -1,
        Some(Weight::Heavy) => 2,
        _ => 0,
    };
    base + adj
}

/// 상의 + 아우터를 합친 보온 점수.
///
/// 하의는 더하지 않는다. 케이스 카탈로그의 온도 라벨이 전부 토르소를 기준으로 달려
/// 있고(10도에 반팔 단독은 실패, 같은 반팔에 파카를 더하면 통과), 하의를 섞으면
/// 치마와 울 슬랙스의 차이가 상의 한 장만큼 커져 라벨과 어긋난다.
///
/// 아우터에 가산점을 주는 이유: 같은 두께라도 바람을 막는 겉옷이 체감에 더 크게
/// 기여한다. 이 값이 없으면 얇은 셔츠 + 얇은 자켓(13도, 라벨 Pass)이 경계로 떨어진다.
fn torso_warmth(top: &OutfitSlot, ctx: &OutfitContext) -> i32 {
    let outer: i32 = ctx
        .slots
        .iter()
        .filter(|s| s.slot == SlotKind::Outer)
        .map(|s| layer_warmth(&s.clothing) + OUTER_BONUS)
        .sum();
    layer_warmth(&top.clothing) + outer
}

/// 기온이 요구하는 토르소 보온 점수.
///
/// 구간 값은 케이스 카탈로그의 온도 라벨에서 역산했다 — 얇은 반팔(2점)이 20도에는
/// 통과하고 18도에는 경계, 10도에는 실패가 되도록 맞춘 격자다.
fn required_torso_warmth(temperature: f64) -> i32 {
    match temperature {
        t if t >= 22.0 => 1,
        t if t >= 20.0 => 2,
        t if t >= 18.0 => 3,
        t if t >= 15.0 => 4,
        t if t >= 13.0 => 5,
        t if t >= 10.0 => 6,
        t if t >= 5.0 => 8,
        _ => 10,
    }
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

    /// 같은 속성의 아우터 슬롯.
    fn outer(weight: Weight, thickness: Thickness) -> OutfitSlot {
        let mut s = top(weight, thickness);
        s.slot = SlotKind::Outer;
        s.clothing.id = "o".into();
        s.clothing.name = "테스트 아우터".into();
        s.clothing.category = "아우터".into();
        s
    }

    fn ctx(slot: OutfitSlot) -> OutfitContext {
        OutfitContext {
            slots: vec![slot],
            situation: None,
        }
    }

    /// 상의 한 장만으로는 보온을 판정할 수 없다는 것이 이 게이트의 전제다.
    /// 같은 상의가 기온에 따라 통과/경계/실패로 갈리는지 고정한다.
    #[test]
    fn a_thin_top_alone_degrades_as_it_gets_colder() {
        // 얇은 원단 + 중간 무게 = 보온 2점.
        let thin = ctx(top(Weight::Mid, Thickness::Thin));
        assert_eq!(compute_today_fit(&thin, 22.0), TodayFitLevel::Pass);
        assert_eq!(compute_today_fit(&thin, 16.0), TodayFitLevel::Borderline);
        assert_eq!(compute_today_fit(&thin, 10.0), TodayFitLevel::Fail);
    }

    /// 두께가 중간이면 같은 기온에서 한 단계 더 버틴다 — 상의가 얇은지 여부를
    /// 깃발로 보지 않고 점수로 보기 때문이다.
    #[test]
    fn a_thicker_top_alone_holds_out_longer() {
        let thin = ctx(top(Weight::Light, Thickness::Thin)); // 1점
        let medium = ctx(top(Weight::Light, Thickness::Medium)); // 3점

        // 18도: 얇은 쪽은 경계, 중간 두께는 통과.
        assert_eq!(compute_today_fit(&thin, 18.0), TodayFitLevel::Borderline);
        assert_eq!(compute_today_fit(&medium, 18.0), TodayFitLevel::Pass);

        // 13도: 얇은 쪽은 실패, 중간 두께는 아직 경계.
        assert_eq!(compute_today_fit(&thin, 13.0), TodayFitLevel::Fail);
        assert_eq!(compute_today_fit(&medium, 13.0), TodayFitLevel::Borderline);
    }

    /// 아우터 없이 미디엄 니트 한 장으로 10도를 넘기는 것은 통과가 아니다.
    ///
    /// 이전 게이트는 "얇거나 가벼운 상의 + 아우터 없음" 이라는 모양만 봤기 때문에,
    /// 두께가 중간이면 기온이 얼마든 통과였다. 라벨로 뒷받침된 판정이 아니라
    /// 규칙의 모양이 그랬을 뿐이다.
    #[test]
    fn a_mid_weight_knit_alone_is_not_enough_at_ten_degrees() {
        let mid_medium = ctx(top(Weight::Mid, Thickness::Medium)); // 4점
        assert_eq!(
            compute_today_fit(&mid_medium, 10.0),
            TodayFitLevel::Borderline
        );
        assert_eq!(compute_today_fit(&mid_medium, 4.0), TodayFitLevel::Fail);
        assert_eq!(compute_today_fit(&mid_medium, 18.0), TodayFitLevel::Pass);
    }

    /// 사용자가 요청한 핵심 케이스: 얇은 이너 + 자켓은 조합 전체로 보온을 맞춘 것이므로
    /// 이너가 얇다는 이유로 깎이지 않아야 한다.
    #[test]
    fn a_thin_inner_under_a_jacket_is_judged_as_a_whole() {
        let thin_inner = top(Weight::Light, Thickness::Thin); // 1점
        let alone = ctx(thin_inner.clone());
        assert_eq!(compute_today_fit(&alone, 12.0), TodayFitLevel::Fail);

        let mut layered = ctx(thin_inner);
        layered.slots.push(outer(Weight::Mid, Thickness::Medium)); // 4+2점
        assert_eq!(compute_today_fit(&layered, 12.0), TodayFitLevel::Pass);
    }

    /// 반대 방향: 아우터가 있다는 사실만으로 통과시키지 않는다.
    /// 얇은 이너에 얇은 윈드브레이커를 더해도 한겨울을 넘길 수는 없다.
    #[test]
    fn a_flimsy_outer_does_not_rescue_a_thin_inner() {
        let mut c = ctx(top(Weight::Light, Thickness::Thin)); // 1점
        c.slots.push(outer(Weight::Light, Thickness::Thin)); // 1+2점
        assert_eq!(compute_today_fit(&c, 12.0), TodayFitLevel::Borderline);
        assert_eq!(compute_today_fit(&c, 2.0), TodayFitLevel::Fail);
    }

    /// 두꺼운 겉옷은 추위에서 게이트를 완전히 들어올린다.
    #[test]
    fn a_heavy_outer_lifts_the_gate() {
        let mut c = ctx(top(Weight::Light, Thickness::Thin));
        c.slots.push(outer(Weight::Heavy, Thickness::Thick));
        assert_eq!(compute_today_fit(&c, 10.0), TodayFitLevel::Pass);
        assert_eq!(compute_today_fit(&c, -5.0), TodayFitLevel::Pass);
    }
}
