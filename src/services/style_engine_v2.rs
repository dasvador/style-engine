//! Style Engine v2 — 3계층 분리 구조의 첫 번째 층(Hard Filter).
//!
//! baseline `style_engine.rs`는 감점 기반 단일 스코어를 계산하지만,
//! v2는 다음 3계층으로 분리한다:
//!   1. Hard Filter — 즉시 탈락 bool (이 파일)
//!   2. Style Score — 서브스코어 기반 미적 점수 (S3에서 구현)
//!   3. Serving Score — recency/diversity/dormant tie-break (S4, `serving_ranker.rs`)
//!
//! 이 파일의 함수들은 기존 style_engine 룰을 복붙하지 않고,
//! "즉시 탈락 성격"의 룰만 pure bool 함수로 재작성한 것이다.
//! 점수화 금지 — `reasons: Vec<HardFilterReason>`만 반환한다.

use serde::Serialize;

use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};

/// 하드필터 탈락 사유 코드. 각 사유는 서로 독립적이며 중복 적재 가능.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum HardFilterReason {
    /// 포멀+스포츠 / 워크 2+ / 밀리터리 2+
    StyleHardConflict,
    /// 아우터 있을 때 top이 '반찬' 혹은 강한 다크+고채도
    StrongInnerViolation,
    /// 밥과 구조템이 모두 없음, 또는 밥이 전부 가벼움 + 반찬 2+
    LackOfStructure,
    /// tones 2개 이상이고 전부 밝음 or 전부 어두움인데 outer/bottom에 구조템 없음
    AllOneTone,
    /// 시즌 데이터가 있는 슬롯 기준 out-of-season 비율 >= 0.8
    SeasonCompleteMismatch,
    /// 3슬롯 이상이 전부 warm인데 구조템도 어두움 anchor도 없음
    WarmMonotoneNoStructure,
    /// 상황이 출근/비즈니스인데 shoes.style == 스포츠
    FormalSituationAthleticShoes,
}

/// 하드필터 실행 결과. `pass == reasons.is_empty()` 불변.
#[derive(Debug, Clone, Serialize)]
pub struct HardFilterResult {
    pub pass: bool,
    pub reasons: Vec<HardFilterReason>,
}

impl HardFilterResult {
    fn from_reasons(reasons: Vec<HardFilterReason>) -> Self {
        Self {
            pass: reasons.is_empty(),
            reasons,
        }
    }
}

/// Today 적합도 3단계. 점수가 아닌 자격 등급.
/// S4에서 온도/아우터/대비 판정 로직과 연결된다.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[allow(dead_code)] // Borderline/Fail은 S4 today-gate 구현 시 사용
pub enum TodayFitLevel {
    Pass,
    Borderline,
    Fail,
}

/// 미적 판단 서브스코어 (S3에서 채워짐).
/// 각 축은 독립적으로 계산되어 디버깅/튜닝 시 어느 축이 망가졌는지 추적 가능해야 한다.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SubScores {
    /// 밥/반찬/구조 밸런스, 밝기 밸런스, 대비
    pub balance: i32,
    /// 스타일/텍스처/world 조화
    pub coherence: i32,
    /// 시즌/온도/활용성
    pub utility: i32,
    /// 신발/가방 적합성
    pub accessory: i32,
}

/// v2 평가 결과. 한 후보(OutfitContext)에 대해 하나 생성.
#[derive(Debug, Clone, Serialize)]
pub struct OutfitEvaluation {
    pub hard: HardFilterResult,
    pub sub: SubScores,
    /// `sub` 합산 기반 미적 점수. S3에서 구현. 현재는 placeholder = 0.
    pub style_score: i32,
    /// Today 적합도 게이트. S4에서 구현. 현재는 placeholder = Pass.
    pub today_fit: TodayFitLevel,
}

// ─────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────

/// 하드필터 실행. 점수화/가중치 금지 — 탈락 사유만 수집.
pub fn run_hard_filter(
    ctx: &OutfitContext,
    current_season: Option<&str>,
) -> HardFilterResult {
    let mut reasons = Vec::new();

    if detect_style_hard_conflict(ctx) {
        reasons.push(HardFilterReason::StyleHardConflict);
    }
    if detect_strong_inner_violation(ctx) {
        reasons.push(HardFilterReason::StrongInnerViolation);
    }
    if detect_lack_of_structure(ctx) {
        reasons.push(HardFilterReason::LackOfStructure);
    }
    if detect_all_one_tone_without_rescue(ctx) {
        reasons.push(HardFilterReason::AllOneTone);
    }
    if let Some(season) = current_season {
        if detect_season_complete_mismatch(ctx, season) {
            reasons.push(HardFilterReason::SeasonCompleteMismatch);
        }
    }
    if detect_warm_monotone_no_structure(ctx) {
        reasons.push(HardFilterReason::WarmMonotoneNoStructure);
    }
    if detect_formal_with_athletic_shoes(ctx) {
        reasons.push(HardFilterReason::FormalSituationAthleticShoes);
    }

    HardFilterResult::from_reasons(reasons)
}

// ─────────────────────────────────────────────────────────────────────────
// Detectors — 각 함수는 pure bool. mutation/scoring 금지.
// ─────────────────────────────────────────────────────────────────────────

fn detect_style_hard_conflict(ctx: &OutfitContext) -> bool {
    // 가방은 hard style conflict에서 완전 제외 — 액세서리일 뿐이므로 false positive 방지.
    // 신발은 포멀+스포츠 체크에서만 포함 (격식 충돌은 시각적 임팩트가 큼).
    let no_bag_styles: Vec<(&SlotKind, &str)> = ctx
        .slots
        .iter()
        .filter(|s| s.slot != SlotKind::Bag)
        .filter_map(|s| s.clothing.style.as_deref().map(|st| (&s.slot, st)))
        .filter(|(_, st)| *st != "베이직")
        .collect();

    // 포멀+스포츠 — 가방 제외, 신발 포함
    let has_formal = no_bag_styles.iter().any(|(_, s)| *s == "포멀");
    let has_sport = no_bag_styles.iter().any(|(_, s)| *s == "스포츠");
    if has_formal && has_sport {
        return true;
    }

    // 워크/밀리터리 — 상의+하의+아우터(큰 슬롯)에서만 카운트. 가방·신발 제외.
    let big_slot_styles: Vec<&str> = no_bag_styles
        .iter()
        .filter(|(slot, _)| matches!(slot, SlotKind::Top | SlotKind::Bottom | SlotKind::Outer))
        .map(|(_, st)| *st)
        .collect();
    for strong in ["워크", "밀리터리"] {
        if big_slot_styles.iter().filter(|s| **s == strong).count() >= 3 {
            return true;
        }
    }
    false
}

fn detect_strong_inner_violation(ctx: &OutfitContext) -> bool {
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    if !has_outer {
        return false;
    }
    let Some(top) = ctx.slots.iter().find(|s| s.slot == SlotKind::Top) else {
        return false;
    };

    let role_is_accent = top.clothing.role.as_deref() == Some("반찬");
    let strong_contrast = top.clothing.tone.as_deref() == Some("어두움")
        && top.clothing.saturation.as_deref() == Some("높음");
    role_is_accent || strong_contrast
}

fn detect_lack_of_structure(ctx: &OutfitContext) -> bool {
    if ctx.slots.is_empty() {
        return false;
    }

    let has_structure = ctx
        .slots
        .iter()
        .any(|s| s.clothing.role.as_deref() == Some("구조템"));
    if has_structure {
        return false;
    }

    let babs: Vec<&OutfitSlot> = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role.as_deref() == Some("밥"))
        .collect();
    let accent_count = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role.as_deref() == Some("반찬"))
        .count();

    if babs.is_empty() {
        // 연결템이 2개 이상이고 가볍지 않으면 어스톤 등 안정 조합으로 간주 → soft로 내림
        let stable_connectors = ctx
            .slots
            .iter()
            .filter(|s| s.clothing.role.as_deref() == Some("연결템"))
            .filter(|s| s.clothing.weight.as_deref() != Some("가벼움"))
            .count();
        if stable_connectors >= 2 {
            return false;
        }
        return true;
    }

    let all_light = babs
        .iter()
        .all(|s| s.clothing.weight.as_deref() == Some("가벼움"));
    all_light && accent_count >= 2
}

fn detect_all_one_tone_without_rescue(ctx: &OutfitContext) -> bool {
    let tones: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.tone.as_deref())
        .collect();
    if tones.len() < 2 {
        return false;
    }

    let all_dark = tones.iter().all(|t| *t == "어두움");
    let all_bright = tones.iter().all(|t| *t == "밝음");
    if !(all_dark || all_bright) {
        return false;
    }

    // Rescue 1: outer/bottom에 구조템
    let has_structural_rescue = ctx
        .slots
        .iter()
        .filter(|s| matches!(s.slot, SlotKind::Outer | SlotKind::Bottom))
        .any(|s| s.clothing.role.as_deref() == Some("구조템"));
    if has_structural_rescue {
        return false;
    }

    // Rescue 2: outer가 존재하면 레이어링 자체가 시각적 분리를 제공.
    // 같은 톤이라도 레이어드 → soft penalty로 내림 (hard fail 금지).
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    if has_outer {
        return false;
    }

    true
}

fn detect_season_complete_mismatch(ctx: &OutfitContext, current_season: &str) -> bool {
    let mut total = 0;
    let mut out = 0;
    for slot in &ctx.slots {
        if slot.seasons.is_empty() {
            continue;
        }
        total += 1;
        if !slot.seasons.iter().any(|s| s == current_season) {
            out += 1;
        }
    }
    // 시즌 데이터가 있는 아이템이 3개 미만이면 판단 보류 (데이터 희소 시 과민 방지)
    if total < 3 {
        return false;
    }
    (out as f32 / total as f32) >= 0.8
}

fn detect_warm_monotone_no_structure(ctx: &OutfitContext) -> bool {
    let temps: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.color_temperature.as_deref())
        .collect();
    if temps.len() < 3 {
        return false;
    }
    if !temps.iter().all(|t| *t == "warm") {
        return false;
    }

    let has_structure = ctx
        .slots
        .iter()
        .any(|s| s.clothing.role.as_deref() == Some("구조템"));
    let has_dark_anchor = ctx
        .slots
        .iter()
        .any(|s| s.clothing.tone.as_deref() == Some("어두움"));

    if has_structure || has_dark_anchor {
        return false;
    }

    // Weak anchor: tone==중간 + formality>=2 + role in {구조,연결} + worlds∩{workwear,minimal} ≠ ∅
    // 어스톤 조합에서 중간톤 워크/미니멀 아이템이 시각적 무게를 줘서 warm 일색을 구제.
    let has_weak_anchor = ctx.slots.iter().any(|s| {
        let tone_mid = s.clothing.tone.as_deref() == Some("중간");
        let formal_enough = s.clothing.formality_level.unwrap_or(0) >= 2;
        let role_ok = matches!(
            s.clothing.role.as_deref(),
            Some("구조템") | Some("연결템")
        );
        let world_ok = s
            .texture_worlds
            .iter()
            .any(|w| w == "workwear" || w == "minimal");
        tone_mid && formal_enough && role_ok && world_ok
    });

    !has_weak_anchor
}

fn detect_formal_with_athletic_shoes(ctx: &OutfitContext) -> bool {
    let Some(situation) = ctx.situation.as_deref() else {
        return false;
    };
    if !matches!(situation, "출근" | "비즈니스") {
        return false;
    }
    ctx.slots
        .iter()
        .filter(|s| s.slot == SlotKind::Shoes)
        .any(|s| s.clothing.style.as_deref() == Some("스포츠"))
}

// ─────────────────────────────────────────────────────────────────────────
// Subscores (Style Score 계층 — S3)
// ─────────────────────────────────────────────────────────────────────────
//
// 원칙:
//   1. hard filter로 승격된 룰은 subscore에서 제외(double-count 금지).
//   2. 각 축은 AXIS_MAX=25로 시작해 soft 감점/보너스 적용 후 [0, 25] clamp.
//   3. v2 style_score = balance + coherence + utility + accessory (0~100).
//   4. 설명 가능성 우선 — 현 단계에서 절대 보정보다 트레이싱 가능한 감점 규칙.

pub const AXIS_MAX: i32 = 25;

/// 4축 subscore를 모두 계산.
pub fn compute_subscores(ctx: &OutfitContext, current_season: Option<&str>) -> SubScores {
    SubScores {
        balance: score_balance(ctx),
        coherence: score_coherence(ctx),
        utility: score_utility(ctx, current_season),
        accessory: score_accessory(ctx),
    }
}

/// 4축 합산 → 0~100 범위 v2 style_score.
pub fn compute_style_score(sub: &SubScores) -> i32 {
    (sub.balance + sub.coherence + sub.utility + sub.accessory).clamp(0, 100)
}

// ─── Axis 1: balance — 밥/반찬, 밝기, 대비, 자연톤(soft) ───
fn score_balance(ctx: &OutfitContext) -> i32 {
    let mut s = AXIS_MAX;

    // 반찬 과다 — 구조 없는 심각 케이스(밥+가벼움+반찬 2+)는 hard filter(LackOfStructure)가 담당.
    // soft: 구조가 있거나 밥이 무거움이어서 hard를 피한 케이스만 감점.
    let accent_count = ctx
        .slots
        .iter()
        .filter(|s| {
            matches!(
                s.clothing.role.as_deref(),
                Some("반찬") | Some("약한반찬")
            )
        })
        .count();
    if accent_count >= 3 {
        s -= 8;
    } else if accent_count >= 2 {
        s -= 4;
    }

    // 밝기/대비: all_dark/all_bright 중 hard filter에 걸리지 않고 구조 구제된 경우만 여기로 옴
    let tones: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.tone.as_deref())
        .collect();
    if tones.len() >= 2 {
        let all_dark = tones.iter().all(|t| *t == "어두움");
        let all_bright = tones.iter().all(|t| *t == "밝음");
        let all_mid = tones.iter().all(|t| *t == "중간");
        if all_dark || all_bright {
            // hard filter(AllOneTone) 미발동 == outer/bottom에 구조템 존재 → 소폭 감점만
            s -= 4;
        } else if all_mid {
            s -= if tones.len() >= 3 { 6 } else { 4 };
        } else {
            let has_bright = tones.iter().any(|t| *t == "밝음");
            let has_dark = tones.iter().any(|t| *t == "어두움");
            if has_bright && has_dark {
                s += 2; // 밝음+어두움 대비 보너스
            }
        }
    }

    // 자연톤 과다 soft — warm 3+ 이지만 구조 또는 어두움 anchor로 hard를 피한 경우만
    let temps: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.color_temperature.as_deref())
        .collect();
    if temps.len() >= 3 && temps.iter().all(|t| *t == "warm") {
        // hard filter(WarmMonotoneNoStructure) 미발동 == has_structure || has_dark_anchor
        s -= 5;
    }

    s.clamp(0, AXIS_MAX)
}

// ─── Axis 2: coherence — texture/world 충돌, 세계관 과잉, 단조로움 ───
fn score_coherence(ctx: &OutfitContext) -> i32 {
    let mut s = AXIS_MAX;

    let worlds: Vec<&str> = ctx
        .slots
        .iter()
        .flat_map(|s| s.texture_worlds.iter().map(|w| w.as_str()))
        .collect();

    // sweat+tailoring — 현 hard filter에 미포함(사용자 지시로 hard 유지) → soft 강 감점
    if worlds.iter().any(|w| *w == "sweat") && worlds.iter().any(|w| *w == "tailoring") {
        s -= 12;
    }
    // outdoor+tailoring 미세 충돌
    if worlds.iter().any(|w| *w == "outdoor") && worlds.iter().any(|w| *w == "tailoring") {
        s -= 5;
    }

    // 밸런싱 페어 보너스
    let has_mil = worlds.iter().any(|w| *w == "military");
    let has_tai = worlds.iter().any(|w| *w == "tailoring");
    let has_work = worlds.iter().any(|w| *w == "workwear");
    if (has_mil && has_tai) || (has_work && has_mil) {
        s += 3;
    }

    // 세계관 과잉 + 강스타일 편중 — 같은 현상의 이중 감점 방지를 위해
    // 두 penalty를 각각 계산한 뒤 max만 적용.
    let mut world_penalty = 0;
    let mut strong_style_penalty = 0;

    // 세계관 과잉: top+bottom이 같은 world + 둘 다 warm + 둘 다 구조템 아님
    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    let bot = ctx.slots.iter().find(|s| s.slot == SlotKind::Bottom);
    if let (Some(t), Some(b)) = (top, bot) {
        let shared_world = !t.texture_worlds.is_empty()
            && t.texture_worlds.iter().any(|w| b.texture_worlds.contains(w));
        let both_warm = t.clothing.color_temperature.as_deref() == Some("warm")
            && b.clothing.color_temperature.as_deref() == Some("warm");
        let neither_structure = t.clothing.role.as_deref() != Some("구조템")
            && b.clothing.role.as_deref() != Some("구조템");
        if shared_world && both_warm && neither_structure {
            world_penalty = 8;
        }
    }

    // 강스타일 편중: 전체 슬롯 기준 워크/밀리터리 카운트
    {
        let all_styles: Vec<&str> = ctx
            .slots
            .iter()
            .filter_map(|s| s.clothing.style.as_deref())
            .collect();
        for strong in ["워크", "밀리터리"] {
            let count = all_styles.iter().filter(|s| **s == strong).count();
            if count >= 3 {
                strong_style_penalty = strong_style_penalty.max(6);
            } else if count >= 2 {
                strong_style_penalty = strong_style_penalty.max(3);
            }
        }
    }

    // 둘 다 발동하면 같은 현상의 이중 감점이므로 max만 적용
    if world_penalty > 0 && strong_style_penalty > 0 {
        s -= world_penalty.max(strong_style_penalty);
    } else {
        s -= world_penalty + strong_style_penalty;
    }

    // flat outfit — 밥/연결템/구조템만으로 구성되고 반찬 없음 + 구조템도 없음
    if ctx.slots.len() >= 2 {
        let all_basic = ctx.slots.iter().all(|s| {
            matches!(
                s.clothing.role.as_deref(),
                Some("밥") | Some("연결템") | Some("구조템")
            )
        });
        let has_accent = ctx.slots.iter().any(|s| {
            matches!(
                s.clothing.role.as_deref(),
                Some("반찬") | Some("약한반찬")
            )
        });
        let has_structure = ctx
            .slots
            .iter()
            .any(|s| s.clothing.role.as_deref() == Some("구조템"));
        if all_basic && !has_accent && !has_structure {
            s -= 5;
        }
    }

    s.clamp(0, AXIS_MAX)
}

// ─── Axis 3: utility — 시즌 soft, 슬롯 역할, 격식 상황 ───
fn score_utility(ctx: &OutfitContext, current_season: Option<&str>) -> i32 {
    let mut s = AXIS_MAX;

    // 시즌 half mismatch (complete mismatch는 hard filter가 담당)
    if let Some(season) = current_season {
        let mut total = 0;
        let mut out = 0;
        for slot in &ctx.slots {
            if slot.seasons.is_empty() {
                continue;
            }
            total += 1;
            if !slot.seasons.iter().any(|s2| s2 == season) {
                out += 1;
            }
        }
        if total > 0 {
            let ratio = out as f32 / total as f32;
            if (0.4..0.8).contains(&ratio) {
                s -= 5;
            }
        }
    }

    // 슬롯 역할 기대치 미스매치
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    let mut mismatch = 0;
    for slot in &ctx.slots {
        let Some(role) = slot.clothing.role.as_deref() else {
            continue;
        };
        let expected = slot.slot.expected_roles(has_outer);
        if !expected.contains(&role) {
            mismatch += 1;
        }
    }
    s -= (mismatch * 4).min(10);

    // 격식 vs 상황
    if let Some(situation) = ctx.situation.as_deref() {
        let levels: Vec<f32> = ctx
            .slots
            .iter()
            .filter_map(|s| s.clothing.formality_level.map(|l| l as f32))
            .collect();
        if !levels.is_empty() {
            let avg = levels.iter().sum::<f32>() / levels.len() as f32;
            let (min_f, max_f) = match situation {
                "출근" | "비즈니스" => (3.0, 5.0),
                "데이트" => (2.0, 4.0),
                "주말" | "가벼운외출" => (1.0, 3.0),
                "캐주얼" | "일상" => (1.0, 2.5),
                _ => (0.0, 10.0),
            };
            if avg < min_f {
                let gap = min_f - avg;
                if gap >= 1.0 {
                    s -= 6;
                } else if gap >= 0.5 {
                    s -= 3;
                }
            } else if avg > max_f {
                let gap = avg - max_f;
                if gap >= 1.0 {
                    s -= 6;
                } else if gap >= 0.5 {
                    s -= 3;
                }
            }
        }
    }

    s.clamp(0, AXIS_MAX)
}

// ─── Axis 4: accessory — 신발/가방 격식·스타일 정렬 ───
fn score_accessory(ctx: &OutfitContext) -> i32 {
    let mut s = AXIS_MAX;

    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    let bottom = ctx.slots.iter().find(|s| s.slot == SlotKind::Bottom);
    let shoes = ctx.slots.iter().find(|s| s.slot == SlotKind::Shoes);
    let bag = ctx.slots.iter().find(|s| s.slot == SlotKind::Bag);

    let clothing_avg_formality: f32 = {
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

    // 신발
    match shoes {
        Some(shoe) => {
            let shoe_formality = shoe.clothing.formality_level.unwrap_or(2) as f32;
            let gap = (shoe_formality - clothing_avg_formality).abs();
            if gap >= 2.0 {
                s -= 6;
            } else if gap >= 1.5 {
                s -= 3;
            } else if gap <= 0.5 {
                s += 1;
            }

            // 스포츠 슈즈 + 격식 옷 (situation이 출근/비즈니스일 땐 hard filter 소관)
            if shoe.clothing.style.as_deref() == Some("스포츠") && clothing_avg_formality >= 3.0 {
                let is_formal_sit = ctx
                    .situation
                    .as_deref()
                    .map(|s| matches!(s, "출근" | "비즈니스"))
                    .unwrap_or(false);
                if !is_formal_sit {
                    s -= 4;
                }
            }
        }
        None => {
            s -= 3;
        }
    }

    // 가방
    if let Some(b) = bag {
        let bag_formality = b.clothing.formality_level.unwrap_or(2) as f32;
        let gap = (bag_formality - clothing_avg_formality).abs();
        if gap >= 2.0 {
            s -= 4;
        } else if gap >= 1.5 {
            s -= 2;
        }
    }

    s.clamp(0, AXIS_MAX)
}
