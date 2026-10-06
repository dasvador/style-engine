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
use crate::models::style_vocab::{Role, Saturation, Silhouette, Style, Tone, Weight};

/// 하드필터 탈락 사유 코드. 각 사유는 서로 독립적이며 중복 적재 가능.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum HardFilterReason {
    /// 포멀+스포츠 / 워크 2+ / 밀리터리 2+
    StyleHardConflict,
    /// 아우터 있을 때 top이 '포인트' 혹은 강한 다크+고채도
    StrongInnerViolation,
    /// 베이스와 구조템이 모두 없음, 또는 베이스가 전부 가벼움 + 포인트 2+
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
    /// 베이스/포인트/구조 밸런스, 밝기 밸런스, 대비
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
pub fn run_hard_filter(ctx: &OutfitContext, current_season: Option<&str>) -> HardFilterResult {
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
    if let Some(season) = current_season
        && detect_season_complete_mismatch(ctx, season)
    {
        reasons.push(HardFilterReason::SeasonCompleteMismatch);
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
    let no_bag_styles: Vec<(&SlotKind, Style)> = ctx
        .slots
        .iter()
        .filter(|s| s.slot != SlotKind::Bag)
        .filter_map(|s| s.clothing.style.map(|st| (&s.slot, st)))
        .filter(|(_, st)| *st != Style::Basic)
        .collect();

    // 포멀+스포츠 — 가방 제외, 신발 포함
    let has_formal = no_bag_styles.iter().any(|(_, s)| *s == Style::Formal);
    let has_sport = no_bag_styles.iter().any(|(_, s)| *s == Style::Sport);
    if has_formal && has_sport {
        return true;
    }

    // 워크/밀리터리 — 상의+하의+아우터(큰 슬롯)에서만 카운트. 가방·신발 제외.
    let big_slot_styles: Vec<Style> = no_bag_styles
        .iter()
        .filter(|(slot, _)| matches!(slot, SlotKind::Top | SlotKind::Bottom | SlotKind::Outer))
        .map(|(_, st)| *st)
        .collect();
    for strong in [Style::Work, Style::Military] {
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

    let role_is_accent = top.clothing.role == Some(Role::Accent);
    let strong_contrast =
        top.clothing.tone == Some(Tone::Dark) && top.clothing.saturation == Some(Saturation::High);
    role_is_accent || strong_contrast
}

fn detect_lack_of_structure(ctx: &OutfitContext) -> bool {
    if ctx.slots.is_empty() {
        return false;
    }

    let has_structure = ctx
        .slots
        .iter()
        .any(|s| s.clothing.role == Some(Role::Structural));
    if has_structure {
        return false;
    }

    let bases: Vec<&OutfitSlot> = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role == Some(Role::Base))
        .collect();
    let accent_count = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role == Some(Role::Accent))
        .count();

    if bases.is_empty() {
        // 연결템이 2개 이상이고 가볍지 않으면 어스톤 등 안정 조합으로 간주 → soft로 내림
        let stable_connectors = ctx
            .slots
            .iter()
            .filter(|s| s.clothing.role == Some(Role::Connector))
            .filter(|s| s.clothing.weight != Some(Weight::Light))
            .count();
        if stable_connectors >= 2 {
            return false;
        }
        return true;
    }

    let all_light = bases
        .iter()
        .all(|s| s.clothing.weight == Some(Weight::Light));
    all_light && accent_count >= 2
}

fn detect_all_one_tone_without_rescue(ctx: &OutfitContext) -> bool {
    let tones: Vec<Tone> = ctx.slots.iter().filter_map(|s| s.clothing.tone).collect();
    if tones.len() < 2 {
        return false;
    }

    let all_dark = tones.iter().all(|t| *t == Tone::Dark);
    let all_bright = tones.iter().all(|t| *t == Tone::Bright);
    if !(all_dark || all_bright) {
        return false;
    }

    // Rescue 1: outer/bottom에 구조템
    let has_structural_rescue = ctx
        .slots
        .iter()
        .filter(|s| matches!(s.slot, SlotKind::Outer | SlotKind::Bottom))
        .any(|s| s.clothing.role == Some(Role::Structural));
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
        .any(|s| s.clothing.role == Some(Role::Structural));
    let has_dark_anchor = ctx
        .slots
        .iter()
        .any(|s| s.clothing.tone == Some(Tone::Dark));

    if has_structure || has_dark_anchor {
        return false;
    }

    // Weak anchor: tone==중간 + formality>=2 + role in {구조,연결} + worlds∩{workwear,minimal} ≠ ∅
    // 어스톤 조합에서 중간톤 워크/미니멀 아이템이 시각적 무게를 줘서 warm 일색을 구제.
    let has_weak_anchor = ctx.slots.iter().any(|s| {
        let tone_mid = s.clothing.tone == Some(Tone::Mid);
        let formal_enough = s.clothing.formality_level.unwrap_or(0) >= 2;
        let role_ok = matches!(
            s.clothing.role,
            Some(Role::Structural) | Some(Role::Connector)
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
        .any(|s| s.clothing.style == Some(Style::Sport))
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

// ─── Axis 1: balance — 베이스/포인트, 밝기, 대비, 자연톤(soft) ───
fn score_balance(ctx: &OutfitContext) -> i32 {
    let mut s = AXIS_MAX;

    // 포인트 과다 — 구조 없는 심각 케이스(베이스+가벼움+포인트 2+)는 hard filter(LackOfStructure)가 담당.
    // soft: 구조가 있거나 베이스가 무거움이어서 hard를 피한 케이스만 감점.
    let accent_count = ctx
        .slots
        .iter()
        .filter(|s| matches!(s.clothing.role, Some(Role::Accent) | Some(Role::SoftAccent)))
        .count();
    if accent_count >= 3 {
        s -= 8;
    } else if accent_count >= 2 {
        s -= 4;
    }

    // 밝기/대비: all_dark/all_bright 중 hard filter에 걸리지 않고 구조 구제된 경우만 여기로 옴
    let tones: Vec<Tone> = ctx.slots.iter().filter_map(|s| s.clothing.tone).collect();
    if tones.len() >= 2 {
        let all_dark = tones.iter().all(|t| *t == Tone::Dark);
        let all_bright = tones.iter().all(|t| *t == Tone::Bright);
        let all_mid = tones.iter().all(|t| *t == Tone::Mid);
        if all_dark || all_bright {
            // hard filter(AllOneTone) 미발동 == outer/bottom에 구조템 존재 → 소폭 감점만
            s -= 4;
        } else if all_mid {
            s -= if tones.len() >= 3 { 6 } else { 4 };
        } else {
            let has_bright = tones.contains(&Tone::Bright);
            let has_dark = tones.contains(&Tone::Dark);
            if has_bright && has_dark {
                s += 2; // 밝음+어두움 대비 보너스
            }
        }
    }

    // 실루엣 볼륨 — 상하의 부피 관계.
    //
    // 비율이 한 번 꺾이는 조합(위가 크고 아래가 좁거나 그 반대)이 가장 또렷하고,
    // 위아래가 같이 커지면 실루엣이 뭉개지고 같이 좁으면 경직된다.
    //
    // 위쪽 부피는 아우터가 있으면 아우터로 본다 — 슬림한 니트 위에 오버사이즈
    // 블레이저를 걸치면 보이는 실루엣은 블레이저 쪽이다.
    //
    // 가점은 이 축이 이미 최대값에서 시작하므로 다른 규칙이 깎아 둔 만큼만 드러난다
    // (아래 밝기 대비 보너스도 같다). 중립 착장과 비율이 또렷한 착장의 점수가 같게
    // 보이는 것은 clamp 때문이고, 버그가 아니다.
    let upper_volume =
        slot_silhouette(ctx, SlotKind::Outer).or(slot_silhouette(ctx, SlotKind::Top));
    let lower_volume = slot_silhouette(ctx, SlotKind::Bottom);
    if let (Some(upper), Some(lower)) = (upper_volume, lower_volume) {
        use Silhouette::{Oversized, Regular, Slim};
        s += match (upper, lower) {
            (Oversized, Oversized) => -6,
            (Slim, Slim) => -3,
            (Oversized, Slim | Regular) | (Slim | Regular, Oversized) => 2,
            // Regular/Relaxed 조합은 중립이다. 여기를 감점하면 실루엣이 아직 일괄
            // regular 인 남성 옷장 전체가 이유 없이 깎인다.
            _ => 0,
        };
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

/// 해당 슬롯의 실루엣 볼륨. 슬롯이 없거나 값이 없으면 `None`.
fn slot_silhouette(ctx: &OutfitContext, slot: SlotKind) -> Option<Silhouette> {
    ctx.slots
        .iter()
        .find(|s| s.slot == slot)
        .and_then(|s| s.clothing.silhouette_volume)
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
    if worlds.contains(&"sweat") && worlds.contains(&"tailoring") {
        s -= 12;
    }
    // outdoor+tailoring 미세 충돌
    if worlds.contains(&"outdoor") && worlds.contains(&"tailoring") {
        s -= 5;
    }

    // 밸런싱 페어 보너스
    let has_mil = worlds.contains(&"military");
    let has_tai = worlds.contains(&"tailoring");
    let has_work = worlds.contains(&"workwear");
    // 밀리터리에 테일러링이나 워크웨어가 섞이면 서로를 눌러주는 조합이 된다.
    if has_mil && (has_tai || has_work) {
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
            && t.texture_worlds
                .iter()
                .any(|w| b.texture_worlds.contains(w));
        let both_warm = t.clothing.color_temperature.as_deref() == Some("warm")
            && b.clothing.color_temperature.as_deref() == Some("warm");
        let neither_structure =
            t.clothing.role != Some(Role::Structural) && b.clothing.role != Some(Role::Structural);
        if shared_world && both_warm && neither_structure {
            world_penalty = 8;
        }
    }

    // 강스타일 편중: 전체 슬롯 기준 워크/밀리터리 카운트
    {
        let all_styles: Vec<Style> = ctx.slots.iter().filter_map(|s| s.clothing.style).collect();
        for strong in [Style::Work, Style::Military] {
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

    // flat outfit — 베이스/연결템/구조템만으로 구성되고 포인트 없음 + 구조템도 없음
    if ctx.slots.len() >= 2 {
        let all_basic = ctx.slots.iter().all(|s| {
            matches!(
                s.clothing.role,
                Some(Role::Base) | Some(Role::Connector) | Some(Role::Structural)
            )
        });
        let has_accent = ctx
            .slots
            .iter()
            .any(|s| matches!(s.clothing.role, Some(Role::Accent) | Some(Role::SoftAccent)));
        let has_structure = ctx
            .slots
            .iter()
            .any(|s| s.clothing.role == Some(Role::Structural));
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
        let Some(role) = slot.clothing.role else {
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
            if shoe.clothing.style == Some(Style::Sport) && clothing_avg_formality >= 3.0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::clothing::Clothing;
    use crate::models::style_vocab::Thickness;
    use chrono::NaiveDateTime;

    fn ts() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-10-07 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// 실루엣만 보는 테스트라 톤·역할을 비워 둔다 — 밝기 대비나 포인트 과다 규칙이
    /// 끼어들면 실루엣 항의 기여를 분리할 수 없다.
    fn garment(slot: SlotKind, silhouette: Option<Silhouette>) -> OutfitSlot {
        OutfitSlot {
            slot,
            clothing: Clothing {
                id: format!("{slot:?}"),
                name: "테스트 아이템".into(),
                category: slot.label().into(),
                gender: None,
                style_mood: None,
                color: None,
                thickness: Thickness::Medium,
                image_url: None,
                tone: None,
                saturation: None,
                style: None,
                weight: None,
                role: None,
                color_temperature: None,
                versatility: None,
                statement_level: None,
                formality_level: None,
                visual_weight: None,
                texture_depth: None,
                visual_weight_v2: None,
                texture_depth_v2: None,
                grounding_score: None,
                shadow_tone: None,
                silhouette_volume: silhouette,
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

    fn balance(upper: Option<Silhouette>, lower: Option<Silhouette>) -> i32 {
        score_balance(&OutfitContext {
            slots: vec![
                garment(SlotKind::Top, upper),
                garment(SlotKind::Bottom, lower),
            ],
            situation: None,
        })
    }

    /// 위아래가 같이 커지면 실루엣이 뭉개지고, 같이 좁으면 경직된다.
    /// 비율이 한 번 꺾인 조합이 가장 높아야 한다.
    #[test]
    fn volume_on_both_halves_scores_below_a_broken_proportion() {
        use Silhouette::{Oversized, Slim};
        let broken = balance(Some(Oversized), Some(Slim));
        let both_big = balance(Some(Oversized), Some(Oversized));
        let both_small = balance(Some(Slim), Some(Slim));

        assert!(both_big < both_small, "{both_big} < {both_small}");
        assert!(both_small < broken, "{both_small} < {broken}");
    }

    /// 값이 없는 것과 regular 는 같은 취급이어야 한다. 남성 옷장 154벌이 아직
    /// 일괄 regular 여서, 여기가 중립이 아니면 남성 추천 전체가 이유 없이 움직인다.
    #[test]
    fn a_regular_or_missing_silhouette_is_neutral() {
        use Silhouette::{Regular, Relaxed};
        let neutral = balance(None, None);
        assert_eq!(balance(Some(Regular), Some(Regular)), neutral);
        assert_eq!(balance(None, Some(Regular)), neutral);
        assert_eq!(balance(Some(Relaxed), Some(Relaxed)), neutral);
        assert_eq!(balance(Some(Regular), Some(Relaxed)), neutral);
    }

    /// 아우터가 있으면 보이는 위쪽 부피는 아우터가 결정한다.
    #[test]
    fn an_outer_decides_the_upper_volume() {
        use Silhouette::{Oversized, Slim};
        // 슬림한 상의 + 슬림한 하의 = 같이 좁은 조합(감점).
        let slim_pair = OutfitContext {
            slots: vec![
                garment(SlotKind::Top, Some(Slim)),
                garment(SlotKind::Bottom, Some(Slim)),
            ],
            situation: None,
        };
        let without = score_balance(&slim_pair);

        // 같은 조합에 오버사이즈 블레이저만 올리면 비율이 꺾인다.
        let mut layered = slim_pair;
        layered
            .slots
            .push(garment(SlotKind::Outer, Some(Oversized)));
        assert!(
            score_balance(&layered) > without,
            "아우터가 위쪽 부피를 덮어써야 한다"
        );
    }

    /// 가점은 다른 규칙이 깎아 둔 만큼만 드러난다. 중립 착장은 이미 축 상한이라
    /// 비율이 또렷해도 점수가 같게 보인다 — clamp 때문이고 버그가 아니다.
    #[test]
    fn the_bonus_only_shows_once_something_else_has_deducted() {
        use Silhouette::{Oversized, Regular, Slim};

        // 상한에서는 가점이 보이지 않는다.
        assert_eq!(
            balance(Some(Oversized), Some(Slim)),
            balance(Some(Regular), Some(Regular))
        );

        // 포인트를 2개 넣어 축을 먼저 깎으면 차이가 드러난다.
        let with_accents = |upper, lower| {
            let mut top = garment(SlotKind::Top, upper);
            top.clothing.role = Some(Role::Accent);
            let mut bottom = garment(SlotKind::Bottom, lower);
            bottom.clothing.role = Some(Role::Accent);
            score_balance(&OutfitContext {
                slots: vec![top, bottom],
                situation: None,
            })
        };
        assert!(
            with_accents(Some(Oversized), Some(Slim)) > with_accents(Some(Regular), Some(Regular))
        );
    }
}
