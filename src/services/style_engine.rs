use chrono::Datelike;

use crate::models::outfit::{
    EvaluationResult, IssueCode, OutfitContext, OutfitSlot, OutfitStrength, RuleProblem, SlotKind,
    StructuredSuggestion, Verdict,
};
use crate::models::style_vocab::{Role, Saturation, Style, Tone, Weight};

// --- 감점 상수 ---
const DEDUCT_BANCHAN_OVERLOAD: i32 = 20;
const DEDUCT_BANCHAN_WRONG_SLOT: i32 = 25; // 이너+하의에 포인트가 있으면 더 심각
const DEDUCT_NO_AXIS: i32 = 15;
const DEDUCT_ALL_DARK: i32 = 15;
const DEDUCT_ALL_BRIGHT: i32 = 15;
const DEDUCT_STYLE_CONFLICT: i32 = 20;
const DEDUCT_INNER_ISSUE: i32 = 10;
// (가방 적합도는 rule_bag 내부에서 동적 계산)
// (신발 적합도는 rule_shoes 내부에서 동적 계산)
const DEDUCT_SEASON_ALL: i32 = 15;
const DEDUCT_SEASON_HALF: i32 = 5;
const DEDUCT_LACK_OF_CONTRAST: i32 = 15;
const DEDUCT_NATURAL_TONE_BASE: i32 = 12;
const DEDUCT_NATURAL_TONE_SEVERE: i32 = 18;
const DEDUCT_TEXTURE_WORLD_CONFLICT: i32 = 15;
const DEDUCT_FORMALITY_MISMATCH: i32 = 12;
const DEDUCT_SLOT_ROLE_MISMATCH: i32 = 8;
const DEDUCT_WORLD_OVERMATCH_MILD: i32 = 8;
const DEDUCT_WORLD_OVERMATCH_SEVERE: i32 = 15;
const DEDUCT_FLAT_OUTFIT: i32 = 7; // 베이스+연결템만으로 구성된 심심한 코디

// 최대 점수 상한 (100은 너무 절대적)
const SCORE_CEILING: i32 = 95;

pub fn evaluate(ctx: &OutfitContext, current_season: Option<&str>) -> EvaluationResult {
    let mut score = SCORE_CEILING;
    let mut problems = Vec::new();
    let mut strengths = Vec::new();

    rule_base_accent(ctx, &mut score, &mut problems, &mut strengths);
    rule_lack_of_structure(ctx, &mut score, &mut problems);
    rule_flat_outfit(ctx, &mut score, &mut problems);
    rule_brightness(ctx, &mut score, &mut problems, &mut strengths);
    rule_lack_of_contrast(ctx, &mut score, &mut problems, &mut strengths);
    rule_natural_tone(ctx, &mut score, &mut problems);
    rule_style_conflict(ctx, &mut score, &mut problems);
    rule_texture_world_conflict(ctx, &mut score, &mut problems, &mut strengths);
    rule_slot_role(ctx, &mut score, &mut problems);
    rule_inner(ctx, &mut score, &mut problems);
    rule_bag(ctx, &mut score, &mut problems, &mut strengths);
    rule_shoes(ctx, &mut score, &mut problems, &mut strengths);
    rule_world_overmatching(ctx, &mut score, &mut problems, &mut strengths);
    rule_formality_situation(ctx, &mut score, &mut problems);
    if let Some(season) = current_season {
        rule_season(ctx, season, &mut score, &mut problems);
    }

    score = score.max(0);

    let suggestions = generate_structured_suggestions(&problems);
    let verdict = Verdict::from_score(score);
    let summary = generate_summary(ctx, &verdict, &problems, &strengths);

    EvaluationResult {
        score,
        verdict,
        problems,
        strengths,
        suggestions,
        summary,
    }
}

/// Rule 1: 포인트(포인트) 과다 — 슬롯 민감
fn rule_base_accent(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let mut accent_count = 0;
    let mut base_count = 0;
    let mut weak_accent_count = 0;
    let mut accent_names = Vec::new();
    let mut accent_in_wrong_slot = false;

    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);

    for slot in &ctx.slots {
        match slot.clothing.role {
            Some(Role::Accent) => {
                accent_count += 1;
                accent_names.push(slot.clothing.name.as_str());
                // 이너(아우터 있을 때) 또는 하의에 포인트는 더 심각
                if (slot.slot == SlotKind::Top && has_outer) || slot.slot == SlotKind::Bottom {
                    accent_in_wrong_slot = true;
                }
            }
            Some(Role::Base) => base_count += 1,
            Some(Role::SoftAccent) => weak_accent_count += 1,
            _ => {}
        }
    }

    if accent_count >= 2 {
        // 포인트가 이너/하의에 있으면 더 큰 감점
        let deduction = if accent_in_wrong_slot {
            DEDUCT_BANCHAN_WRONG_SLOT
        } else {
            DEDUCT_BANCHAN_OVERLOAD
        };
        *score -= deduction;
        problems.push(RuleProblem {
            code: IssueCode::TooManyAccents,
            rule: "베이스/포인트 밸런스".to_string(),
            deduction,
            detail: format!(
                "포인트(포인트) 아이템이 {}개로 과합니다: {}{}",
                accent_count,
                accent_names.join(", "),
                if accent_in_wrong_slot {
                    " (이너/하의에 포인트가 있어 더 불안정)"
                } else {
                    ""
                }
            ),
        });
    }

    // 좋은 밸런스
    if base_count >= 1 && accent_count == 1 && accent_count + weak_accent_count <= 1 {
        strengths.push(OutfitStrength {
            rule: "베이스/포인트 밸런스".to_string(),
            detail: "베이스와 포인트의 비율이 적절합니다. 포인트 아이템이 잘 살아납니다"
                .to_string(),
        });
    }

    if base_count >= 1 && accent_count == 0 && weak_accent_count >= 1 {
        strengths.push(OutfitStrength {
            rule: "베이스/포인트 밸런스".to_string(),
            detail: "은은한 포인트로 차분하면서도 개성 있는 조합입니다".to_string(),
        });
    }
}

/// Rule 2: 중심축 부재 (LackOfStructure 재정의)
/// "베이스 없음"이 아니라 "중심축 없음"으로 판단
fn rule_lack_of_structure(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    if ctx.slots.is_empty() {
        return;
    }

    let has_structure = ctx
        .slots
        .iter()
        .any(|s| s.clothing.role == Some(Role::Structural));

    let base_count = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role == Some(Role::Base))
        .count();

    let accent_count = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role == Some(Role::Accent))
        .count();

    // 베이스가 있더라도 전부 가벼우면 중심축 약함
    let all_base_light = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.role == Some(Role::Base))
        .all(|s| s.clothing.weight == Some(Weight::Light));

    let warm_count = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.color_temperature.as_deref())
        .filter(|t| *t == "warm")
        .count();
    let total_with_temp = ctx
        .slots
        .iter()
        .filter(|s| s.clothing.color_temperature.is_some())
        .count();
    let warm_dominant = total_with_temp >= 3 && warm_count == total_with_temp;

    // 중심축 없음 조건:
    // 1) 베이스도 구조템도 없음
    // 2) 베이스는 있지만 전부 가벼움 + (accent 2개 이상 또는 warm 편중)
    let no_axis = if !has_structure && base_count == 0 {
        true
    } else {
        !has_structure && base_count > 0 && all_base_light && (accent_count >= 2 || warm_dominant)
    };

    if no_axis {
        *score -= DEDUCT_NO_AXIS;
        problems.push(RuleProblem {
            code: IssueCode::LackOfStructure,
            rule: "중심축 부재".to_string(),
            deduction: DEDUCT_NO_AXIS,
            detail:
                "코디의 중심축이 부족합니다. 시각적 무게감을 잡아줄 베이스 또는 구조템이 필요합니다"
                    .to_string(),
        });
    }
}

/// Rule 2b: 베이스+연결템만으로 구성된 심심한 코디
/// 안정적이지만 스타일적 재미가 부족한 경우 약하게 감점
fn rule_flat_outfit(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    if ctx.slots.len() < 2 {
        return;
    }

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

    // 포인트 0개 + 구조템 0개 = 베이스+연결템만 → 심심한 코디
    if all_basic && !has_accent && !has_structure {
        *score -= DEDUCT_FLAT_OUTFIT;
        problems.push(RuleProblem {
            code: IssueCode::LackOfStructure,
            rule: "코디 단조로움".to_string(),
            deduction: DEDUCT_FLAT_OUTFIT,
            detail: "안정적이지만 포인트나 구조감이 없어 단조롭습니다".to_string(),
        });
    }
}

/// Rule 3: 밝기 밸런스
fn rule_brightness(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let tones: Vec<Tone> = ctx.slots.iter().filter_map(|s| s.clothing.tone).collect();

    if tones.len() < 2 {
        return;
    }

    let all_dark = tones.iter().all(|t| *t == Tone::Dark);
    let all_bright = tones.iter().all(|t| *t == Tone::Bright);

    if all_dark {
        *score -= DEDUCT_ALL_DARK;
        problems.push(RuleProblem {
            code: IssueCode::LackOfContrast,
            rule: "밝기 밸런스".to_string(),
            deduction: DEDUCT_ALL_DARK,
            detail: "모든 아이템이 어두운 톤이라 답답해 보일 수 있습니다".to_string(),
        });
    } else if all_bright {
        *score -= DEDUCT_ALL_BRIGHT;
        problems.push(RuleProblem {
            code: IssueCode::LackOfContrast,
            rule: "밝기 밸런스".to_string(),
            deduction: DEDUCT_ALL_BRIGHT,
            detail: "모든 아이템이 밝은 톤이라 흐려 보일 수 있습니다".to_string(),
        });
    } else {
        let has_dark = tones.contains(&Tone::Dark);
        let has_bright = tones.contains(&Tone::Bright);
        if has_dark && has_bright {
            strengths.push(OutfitStrength {
                rule: "밝기 밸런스".to_string(),
                detail: "밝음과 어두움의 대비가 잘 잡혀있어 시각적으로 깔끔합니다".to_string(),
            });
        }
    }
}

/// Rule 4: 대비 부족 — 2~3아이템에서도 감지
fn rule_lack_of_contrast(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let items: Vec<(Tone, Saturation)> = ctx
        .slots
        .iter()
        .filter_map(|s| {
            let tone = s.clothing.tone?;
            let sat = s.clothing.saturation?;
            Some((tone, sat))
        })
        .collect();

    if items.len() < 2 {
        return;
    }

    let has_bright = items.iter().any(|(t, _)| *t == Tone::Bright);
    let has_dark = items.iter().any(|(t, _)| *t == Tone::Dark);
    let all_mid = items.iter().all(|(t, _)| *t == Tone::Mid);

    // Case 1: 톤과 채도가 모두 동일하고 중간이면 밋밋
    let first = items[0];
    let all_same = items.iter().all(|i| *i == first);
    if all_same && first.0 == Tone::Mid && first.1 == Saturation::Mid {
        *score -= DEDUCT_LACK_OF_CONTRAST;
        problems.push(RuleProblem {
            code: IssueCode::LackOfContrast,
            rule: "대비 부족".to_string(),
            deduction: DEDUCT_LACK_OF_CONTRAST,
            detail: "톤과 채도가 모두 중간이라 밋밋해 보일 수 있습니다".to_string(),
        });
        return;
    }

    // Case 2: 밝음도 어두움도 없이 중간 톤만 → 대비 약함 (2~3아이템에서도 감지)
    if all_mid && items.len() >= 2 {
        let deduction = if items.len() >= 3 { 10 } else { 6 };
        *score -= deduction;
        problems.push(RuleProblem {
            code: IssueCode::LackOfContrast,
            rule: "대비 부족".to_string(),
            deduction,
            detail:
                "모든 아이템이 중간 톤이라 대비가 약합니다. 밝거나 어두운 아이템을 하나 넣어보세요"
                    .to_string(),
        });
        return;
    }

    // 강점: 다양한 톤
    let unique_tones: std::collections::HashSet<Tone> = items.iter().map(|(t, _)| *t).collect();
    if unique_tones.len() >= 3 {
        strengths.push(OutfitStrength {
            rule: "대비".to_string(),
            detail: "다양한 톤이 조화롭게 구성되어 입체감이 있습니다".to_string(),
        });
    } else if has_bright && has_dark {
        // 이건 brightness rule에서 이미 강점 처리하므로 여기선 skip
    }
}

/// Rule 5: 자연톤 과다 (조건부 가중치)
fn rule_natural_tone(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let temps: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.color_temperature.as_deref())
        .collect();

    if temps.len() < 3 {
        return;
    }

    let warm_count = temps.iter().filter(|t| **t == "warm").count();
    if warm_count != temps.len() {
        return;
    }

    // 조건부 가중치: warm 3+ & structure/구조템 없으면 더 세게
    let has_structure = ctx
        .slots
        .iter()
        .any(|s| s.clothing.role == Some(Role::Structural));

    let has_cool_anchor = ctx
        .slots
        .iter()
        .any(|s| s.clothing.tone == Some(Tone::Dark));

    let deduction = if !has_structure && !has_cool_anchor {
        DEDUCT_NATURAL_TONE_SEVERE
    } else {
        DEDUCT_NATURAL_TONE_BASE
    };

    *score -= deduction;
    problems.push(RuleProblem {
        code: IssueCode::TooMuchNaturalTone,
        rule: "자연톤 과다".to_string(),
        deduction,
        detail: if deduction == DEDUCT_NATURAL_TONE_SEVERE {
            "모든 아이템이 웜톤이고 중심을 잡아줄 구조가 없어 상당히 흐려 보입니다".to_string()
        } else {
            "모든 아이템이 웜톤(자연색)이라 전체적으로 흐려 보일 수 있습니다".to_string()
        },
    });
}

/// Rule 6: 스타일 충돌 — 밀리터리+포멀(tailoring)은 밸런싱 페어로 재분류
fn rule_style_conflict(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let styles: Vec<Style> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.style)
        .filter(|s| *s != Style::Basic)
        .collect();

    // 진짜 충돌만 남김 (밀리터리+포멀은 제거 — 밸런싱 페어)
    let hard_conflicts = [(Style::Formal, Style::Sport)];

    for (a, b) in &hard_conflicts {
        let has_a = styles.iter().any(|s| s == a);
        let has_b = styles.iter().any(|s| s == b);
        if has_a && has_b {
            *score -= DEDUCT_STYLE_CONFLICT;
            problems.push(RuleProblem {
                code: IssueCode::StyleConflict,
                rule: "스타일 충돌".to_string(),
                deduction: DEDUCT_STYLE_CONFLICT,
                detail: format!("{} + {} 조합은 어색할 수 있습니다", a, b),
            });
            return;
        }
    }

    let strong_styles = [Style::Work, Style::Military];
    for ss in &strong_styles {
        let count = styles.iter().filter(|s| **s == *ss).count();
        if count >= 2 {
            *score -= DEDUCT_STYLE_CONFLICT;
            problems.push(RuleProblem {
                code: IssueCode::StyleConflict,
                rule: "스타일 충돌".to_string(),
                deduction: DEDUCT_STYLE_CONFLICT,
                detail: format!("{} 스타일 아이템이 {}개로 과합니다", ss, count),
            });
            return;
        }
    }
}

/// Rule 7: 텍스처 월드 충돌 — military+tailoring은 밸런싱 페어
fn rule_texture_world_conflict(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let all_worlds: Vec<&str> = ctx
        .slots
        .iter()
        .flat_map(|s| s.texture_worlds.iter().map(|w| w.as_str()))
        .collect();

    if all_worlds.is_empty() {
        return;
    }

    // 진짜 충돌 (sweat+tailoring은 여전히 충돌)
    let hard_conflicts = [("sweat", "tailoring")];
    // 약한 충돌 (outdoor+tailoring: 미세 감점)
    let mild_conflicts = [("outdoor", "tailoring")];
    // 밸런싱 페어 (충돌이 아닌 보완 관계)
    let balance_pairs = [("military", "tailoring"), ("workwear", "military")];

    for (a, b) in &hard_conflicts {
        let has_a = all_worlds.iter().any(|w| w == a);
        let has_b = all_worlds.iter().any(|w| w == b);
        if has_a && has_b {
            *score -= DEDUCT_TEXTURE_WORLD_CONFLICT;
            problems.push(RuleProblem {
                code: IssueCode::TextureWorldConflict,
                rule: "텍스처 충돌".to_string(),
                deduction: DEDUCT_TEXTURE_WORLD_CONFLICT,
                detail: format!("{} + {} 텍스처가 섞여 어색할 수 있습니다", a, b),
            });
            return;
        }
    }

    for (a, b) in &mild_conflicts {
        let has_a = all_worlds.iter().any(|w| w == a);
        let has_b = all_worlds.iter().any(|w| w == b);
        if has_a && has_b {
            let deduction = 5;
            *score -= deduction;
            problems.push(RuleProblem {
                code: IssueCode::TextureWorldConflict,
                rule: "텍스처 미세 충돌".to_string(),
                deduction,
                detail: format!("{} + {} 텍스처가 약간 어색할 수 있습니다", a, b),
            });
            return;
        }
    }

    // 밸런싱 페어 강점
    for (a, b) in &balance_pairs {
        let has_a = all_worlds.iter().any(|w| w == a);
        let has_b = all_worlds.iter().any(|w| w == b);
        if has_a && has_b {
            let label = if (*a == "military" && *b == "tailoring")
                || (*a == "tailoring" && *b == "military")
            {
                "밀리터리와 테일러링이 거친 + 정제의 밸런스를 만들어냅니다"
            } else {
                "워크웨어와 밀리터리가 자연스럽게 어우러지는 아메카지 스타일입니다"
            };
            strengths.push(OutfitStrength {
                rule: "텍스처 조화".to_string(),
                detail: label.to_string(),
            });
            return;
        }
    }
}

/// Rule 8: 슬롯 기대 역할 미스매치
fn rule_slot_role(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);

    for slot in &ctx.slots {
        let role = match slot.clothing.role {
            Some(r) => r,
            None => continue,
        };

        let expected = slot.slot.expected_roles(has_outer);
        if !expected.contains(&role) {
            *score -= DEDUCT_SLOT_ROLE_MISMATCH;
            problems.push(RuleProblem {
                code: IssueCode::SlotRoleMismatch,
                rule: "슬롯 역할".to_string(),
                deduction: DEDUCT_SLOT_ROLE_MISMATCH,
                detail: format!(
                    "{}({})의 역할이 '{}'인데, {} 자리에는 {} 역할이 더 적합합니다",
                    slot.clothing.name,
                    slot.slot.label(),
                    role,
                    slot.slot.label(),
                    expected
                        .iter()
                        .map(|r| r.as_str())
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            });
        }
    }
}

/// Rule 9: 이너 규칙
fn rule_inner(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    if !has_outer {
        return;
    }

    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    if let Some(top_slot) = top {
        let is_strong_inner = top_slot.clothing.role == Some(Role::Accent)
            || (top_slot.clothing.tone == Some(Tone::Dark)
                && top_slot.clothing.saturation == Some(Saturation::High));

        if is_strong_inner {
            *score -= DEDUCT_INNER_ISSUE;
            problems.push(RuleProblem {
                code: IssueCode::StrongInner,
                rule: "이너 규칙".to_string(),
                deduction: DEDUCT_INNER_ISSUE,
                detail: format!(
                    "이너({})가 너무 강해서 아우터와 경쟁합니다",
                    top_slot.clothing.name
                ),
            });
        }
    }
}

/// Rule 10: 가방 규칙 — severity-aware
/// Rule 10a: 가방 적합도 (양방향 adjustment: -8 ~ +5)
/// 신발보다 낮은 임팩트 — 코디 마무리 보정 역할
fn rule_bag(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let bag_slot = match ctx.slots.iter().find(|s| s.slot == SlotKind::Bag) {
        Some(b) => b,
        None => return,
    };

    let shoes_slot = ctx.slots.iter().find(|s| s.slot == SlotKind::Shoes);
    let bag_name = &bag_slot.clothing.name;
    let bag_role = bag_slot.clothing.role.map(|v| v.as_str()).unwrap_or("");
    let bag_style = bag_slot.clothing.style.unwrap_or(Style::Basic);
    let bag_tone = bag_slot.clothing.tone.unwrap_or(Tone::Mid);

    let mut adjustment = 0i32;
    let mut reasons_good: Vec<String> = Vec::new();
    let mut reasons_bad: Vec<String> = Vec::new();

    // ─── 1. 역할 적합도 ───
    let other_accent_count = ctx
        .slots
        .iter()
        .filter(|s| s.slot != SlotKind::Bag)
        .filter(|s| matches!(s.clothing.role, Some(Role::Accent) | Some(Role::SoftAccent)))
        .count();

    match bag_role {
        "구조템" => {
            adjustment += 3;
            reasons_good.push("정리 역할로 코디를 안정시킵니다".to_string());
        }
        "연결템" => {
            adjustment += 2;
            reasons_good.push("자연스러운 연결 역할".to_string());
        }
        "베이스" => {
            adjustment += 1;
        }
        "포인트" => {
            if other_accent_count >= 1 {
                adjustment -= 8;
                reasons_bad.push("가방까지 포인트라 시선이 분산됩니다".to_string());
            } else {
                adjustment -= 3;
                reasons_bad.push("가방이 다소 눈에 띕니다".to_string());
            }
        }
        "약한포인트" if other_accent_count >= 2 => {
            adjustment -= 5;
            reasons_bad.push("포인트 아이템이 이미 많습니다".to_string());
        }
        _ => {}
    }

    // ─── 2. 스타일 호환 ───
    let dominant_styles: Vec<Style> = ctx
        .slots
        .iter()
        .filter(|s| s.slot != SlotKind::Bag)
        .filter_map(|s| s.clothing.style)
        .filter(|s| *s != Style::Basic)
        .collect();

    if bag_style == Style::Basic {
        // 베이직 가방은 항상 안전
    } else if let Some(dominant) = dominant_styles.first() {
        if bag_style == *dominant {
            adjustment += 2;
            reasons_good.push(format!("{} 스타일 통일", bag_style));
        } else {
            let clash = (*dominant == Style::Formal
                && (bag_style == Style::Sport || bag_style == Style::Military))
                || (*dominant == Style::Military && bag_style == Style::Formal);
            if clash {
                adjustment -= 5;
                reasons_bad.push(format!(
                    "가방({})이 코디({})와 스타일 불일치",
                    bag_style, dominant
                ));
            }
        }
    }

    // ─── 3. 색상 조화: 하의와 톤 매치 ───
    let bottom_tone = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Bottom)
        .and_then(|s| s.clothing.tone)
        .unwrap_or(Tone::Mid);

    if bag_tone == bottom_tone || bag_tone == Tone::Mid {
        adjustment += 1;
    }

    // ─── 4. 신발과의 관계: 둘 다 포인트면 감점 ───
    if let Some(shoes) = shoes_slot {
        let shoes_accent = matches!(
            shoes.clothing.role,
            Some(Role::Accent) | Some(Role::SoftAccent)
        );
        let bag_accent = matches!(bag_role, "포인트" | "약한포인트");
        if shoes_accent && bag_accent {
            adjustment -= 3;
            reasons_bad.push("신발과 가방 모두 포인트라 과합니다".to_string());
        }
    }

    // ─── 5. 존재감 과부하: 강한 가방 + 강한 아우터/신발 ───
    let bag_statement = bag_slot.clothing.statement_level.unwrap_or(1);
    if bag_statement >= 3 {
        let outer_strong = ctx
            .slots
            .iter()
            .find(|s| s.slot == SlotKind::Outer)
            .and_then(|s| s.clothing.statement_level)
            .unwrap_or(0)
            >= 3;
        let shoes_strong = shoes_slot
            .and_then(|s| s.clothing.statement_level)
            .unwrap_or(0)
            >= 3;

        if outer_strong || shoes_strong {
            adjustment -= 4;
            reasons_bad.push("가방 존재감이 강한데 아우터/신발도 눈에 띕니다".to_string());
        }
    }

    // ─── 6. 밝기 밸런스 보정 ───
    let top_tone = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Top)
        .and_then(|s| s.clothing.tone)
        .unwrap_or(Tone::Mid);

    let outfit_all_dark = top_tone == Tone::Dark && bottom_tone == Tone::Dark;
    let outfit_all_bright = top_tone == Tone::Bright && bottom_tone == Tone::Bright;

    if outfit_all_dark && bag_tone == Tone::Bright {
        adjustment += 2;
        reasons_good.push("어두운 코디에 밝은 가방으로 포인트".to_string());
    } else if outfit_all_bright && bag_tone == Tone::Dark {
        adjustment += 2;
        reasons_good.push("밝은 코디에 어두운 가방으로 앵커".to_string());
    }

    // ─── 적용: clamp -8 ~ +5 ───
    adjustment = adjustment.clamp(-8, 5);

    if adjustment > 0 {
        *score += adjustment;
        if *score > SCORE_CEILING {
            *score = SCORE_CEILING;
        }
        if !reasons_good.is_empty() {
            strengths.push(OutfitStrength {
                rule: "가방 적합도".to_string(),
                detail: format!(
                    "{}이(가) {} (+{}점)",
                    bag_name,
                    reasons_good.join(", "),
                    adjustment
                ),
            });
        }
    } else if adjustment < -2 {
        *score += adjustment;
        if let Some(reason) = reasons_bad.first() {
            problems.push(RuleProblem {
                code: IssueCode::BagConflict,
                rule: "가방 적합도".to_string(),
                deduction: -adjustment,
                detail: reason.clone(),
            });
        }
    }
}

/// Rule 10b: 신발 밸런스
/// Rule 10b: 신발 적합도 (양방향 adjustment: +10 ~ -15)
fn rule_shoes(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let shoes_slot = match ctx.slots.iter().find(|s| s.slot == SlotKind::Shoes) {
        Some(s) => s,
        None => return,
    };

    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    let bottom = ctx.slots.iter().find(|s| s.slot == SlotKind::Bottom);
    let shoe_tone = shoes_slot.clothing.tone.unwrap_or(Tone::Mid);
    let shoe_role = shoes_slot.clothing.role.map(|v| v.as_str()).unwrap_or("");
    let shoe_style = shoes_slot.clothing.style.unwrap_or(Style::Basic);
    let shoe_name = &shoes_slot.clothing.name;

    let mut adjustment = 0i32;
    let mut reasons_good: Vec<String> = Vec::new();
    let mut reasons_bad: Vec<String> = Vec::new();

    // ─── 1. 역할 적합도 ───
    match shoe_role {
        "구조템" => {
            adjustment += 5;
            reasons_good.push("안정적인 구조 역할".to_string());
        }
        "연결템" => {
            adjustment += 3;
            reasons_good.push("자연스러운 연결 역할".to_string());
        }
        "베이스" => {
            adjustment += 1;
        }
        "포인트" | "약한포인트" => {
            // 코디에 이미 포인트가 있으면 과다
            let has_other_accent = ctx
                .slots
                .iter()
                .filter(|s| s.slot != SlotKind::Shoes)
                .any(|s| matches!(s.clothing.role, Some(Role::Accent) | Some(Role::SoftAccent)));
            if has_other_accent {
                adjustment -= 10;
                reasons_bad.push("신발까지 포인트라 산만합니다".to_string());
            } else {
                adjustment -= 3;
                reasons_bad.push("신발이 포인트 역할입니다".to_string());
            }
        }
        _ => {}
    }

    // ─── 2. 대비 보강 ───
    let top_tone = top.and_then(|s| s.clothing.tone).unwrap_or(Tone::Mid);
    let bottom_tone = bottom.and_then(|s| s.clothing.tone).unwrap_or(Tone::Mid);

    let outfit_low_contrast = top_tone == bottom_tone;

    if outfit_low_contrast {
        // 코디 대비가 약한데 신발이 다른 톤이면 보강
        if shoe_tone != top_tone {
            adjustment += 5;
            reasons_good.push("코디 대비를 보강합니다".to_string());
        }
    } else {
        // 코디 대비가 충분한데 신발이 한쪽과 맞으면 안정감
        if shoe_tone == bottom_tone || shoe_tone == Tone::Mid {
            adjustment += 2;
        }
    }

    // 전부 밝은 코디에 어두운 신발 = 강한 보강
    if top_tone == Tone::Bright && bottom_tone == Tone::Bright && shoe_tone == Tone::Dark {
        adjustment += 3; // 위 대비 보강과 합산되면 +8
        reasons_good.push("밝은 코디에 시각적 앵커".to_string());
    }

    // ─── 3. 스타일 호환 ───
    let dominant_styles: Vec<Style> = ctx
        .slots
        .iter()
        .filter(|s| s.slot != SlotKind::Shoes)
        .filter_map(|s| s.clothing.style)
        .filter(|s| *s != Style::Basic)
        .collect();

    if shoe_style == Style::Basic {
        // 베이직 신발은 항상 안전
        adjustment += 1;
    } else if let Some(dominant) = dominant_styles.first() {
        if shoe_style == *dominant {
            // 동일 스타일 = 좋은 매치
            adjustment += 3;
            reasons_good.push(format!("{} 스타일 통일", shoe_style));
        } else {
            // 스타일 충돌 체크
            let clash = (*dominant == Style::Formal && shoe_style == Style::Sport)
                || (*dominant == Style::Military && shoe_style == Style::Formal);
            if clash {
                adjustment -= 8;
                reasons_bad.push(format!(
                    "{}({})이 코디({})와 스타일 불일치",
                    shoe_name, shoe_style, dominant
                ));
            }
        }
    }

    // ─── 4. 시즌 적합 ───
    if !shoes_slot.seasons.is_empty() {
        let current = match chrono::Local::now().month() {
            3..=5 => "봄",
            6..=8 => "여름",
            9..=11 => "가을",
            _ => "겨울",
        };
        if !shoes_slot.seasons.iter().any(|s| s == current) {
            adjustment -= 4;
            reasons_bad.push("현재 시즌에 맞지 않는 신발".to_string());
        }
    }

    // ─── 적용 ───
    // 클램프: -15 ~ +10
    adjustment = adjustment.clamp(-15, 10);

    if adjustment > 0 {
        *score += adjustment;
        // ceiling 유지
        if *score > SCORE_CEILING {
            *score = SCORE_CEILING;
        }
        if !reasons_good.is_empty() {
            strengths.push(OutfitStrength {
                rule: "신발 적합도".to_string(),
                detail: format!(
                    "{}이(가) {} (+{}점)",
                    shoe_name,
                    reasons_good.join(", "),
                    adjustment
                ),
            });
        }
    } else if adjustment < -3 {
        *score += adjustment; // 음수이므로 감점
        if let Some(reason) = reasons_bad.first() {
            problems.push(RuleProblem {
                code: IssueCode::SlotRoleMismatch,
                rule: "신발 적합도".to_string(),
                deduction: -adjustment,
                detail: reason.clone(),
            });
        }
    }
    // -3 ~ 0: 무시 (중립)
}

/// Rule 10c: 세계관 과잉 매칭 — 밀리터리/워크웨어/아웃도어 일반화
fn rule_world_overmatching(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let outer = ctx.slots.iter().find(|s| s.slot == SlotKind::Outer);
    let bottom = ctx.slots.iter().find(|s| s.slot == SlotKind::Bottom);

    let (outer_slot, bottom_slot) = match (outer, bottom) {
        (Some(o), Some(b)) => (o, b),
        _ => return,
    };

    // 아우터가 테마성 강한 스타일인지 (밀리터리/워크/아웃도어)
    let thematic_worlds = ["military", "workwear", "outdoor"];
    let outer_thematic: Vec<&str> = outer_slot
        .texture_worlds
        .iter()
        .filter(|w| thematic_worlds.contains(&w.as_str()))
        .map(|w| w.as_str())
        .collect();

    let outer_style = outer_slot.clothing.style;
    let is_thematic_outer = !outer_thematic.is_empty()
        || outer_style == Some(Style::Military)
        || outer_style == Some(Style::Work);

    if !is_thematic_outer {
        return;
    }

    // 하의 속성
    let bottom_color = bottom_slot.clothing.color.as_deref().unwrap_or("");
    let bottom_temp = bottom_slot
        .clothing
        .color_temperature
        .as_deref()
        .unwrap_or("");
    let bottom_tone = bottom_slot.clothing.tone;
    let bottom_role = bottom_slot.clothing.role;

    // 인디고/다크 계열은 안정화 앵커 역할
    let anchor_colors = ["인디고", "블랙", "차콜", "다크", "네이비"];
    let is_anchor_bottom =
        anchor_colors.iter().any(|c| bottom_color.contains(c)) || bottom_tone == Some(Tone::Dark);

    if is_anchor_bottom {
        strengths.push(OutfitStrength {
            rule: "세계관 밸런스".to_string(),
            detail: format!(
                "{}가 아우터의 안정적인 앵커 역할을 합니다",
                bottom_slot.clothing.name
            ),
        });
        return;
    }

    // 자연톤/warm 하의 감지
    let natural_colors = [
        "올리브",
        "카키",
        "카멜",
        "베이지",
        "탄",
        "브라운",
        "샌드",
        "러스트",
    ];
    let is_natural_bottom = natural_colors.iter().any(|c| bottom_color.contains(c))
        || (bottom_temp == "warm" && bottom_tone == Some(Tone::Mid));

    if !is_natural_bottom {
        return;
    }

    // 하의도 같은 테마 세계관을 공유하는지
    let bottom_thematic: Vec<&str> = bottom_slot
        .texture_worlds
        .iter()
        .filter(|w| thematic_worlds.contains(&w.as_str()))
        .map(|w| w.as_str())
        .collect();
    let shared_world = outer_thematic.iter().any(|w| bottom_thematic.contains(w));

    // 톤 대비가 없는지
    let outer_temp = outer_slot
        .clothing
        .color_temperature
        .as_deref()
        .unwrap_or("");
    let outer_tone = outer_slot.clothing.tone;
    let both_warm_mid = (outer_temp == "warm" || outer_tone == Some(Tone::Mid))
        && (bottom_temp == "warm" || bottom_tone == Some(Tone::Mid));

    let is_structure = bottom_role == Some(Role::Structural);

    // 심각도 결정
    let deduction = if shared_world && both_warm_mid && !is_structure {
        // 같은 세계관 + 같은 무드 + 구조 없음 = severe
        DEDUCT_WORLD_OVERMATCH_SEVERE
    } else if both_warm_mid && !is_structure {
        // 다른 세계관이지만 warm 편중 + 구조 없음
        DEDUCT_WORLD_OVERMATCH_MILD + 4 // -12
    } else if is_structure {
        DEDUCT_WORLD_OVERMATCH_MILD / 2 // 구조템이면 절반
    } else {
        DEDUCT_WORLD_OVERMATCH_MILD
    };

    if deduction > 0 {
        let theme_label =
            if outer_style == Some(Style::Military) || outer_thematic.contains(&"military") {
                "밀리터리"
            } else if outer_style == Some(Style::Work) || outer_thematic.contains(&"workwear") {
                "워크웨어"
            } else {
                "테마"
            };

        *score -= deduction;
        problems.push(RuleProblem {
            code: IssueCode::WorldOvermatching,
            rule: "세계관 과잉".to_string(),
            deduction,
            detail: format!(
                "{} 아우터({})와 자연톤 하의({})가 너무 비슷한 무드라 단조로워 보일 수 있습니다. 인디고 데님이나 차콜 팬츠로 대비를 만들어보세요",
                theme_label,
                outer_slot.clothing.name,
                bottom_slot.clothing.name,
            ),
        });
    }
}

/// Rule 11: 격식 수준 vs 상황
fn rule_formality_situation(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let situation = match ctx.situation.as_deref() {
        Some(s) => s,
        None => return,
    };

    let levels: Vec<i8> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.formality_level)
        .collect();

    if levels.is_empty() {
        return;
    }

    let avg: f32 = levels.iter().map(|l| *l as f32).sum::<f32>() / levels.len() as f32;

    let (min_formality, max_formality) = match situation {
        "출근" | "비즈니스" => (3.0, 5.0),
        "데이트" => (2.0, 4.0),
        "주말" | "가벼운외출" => (1.0, 3.0),
        "캐주얼" | "일상" => (1.0, 2.5),
        _ => return,
    };

    if avg < min_formality {
        // 격차가 클수록 추가 감점
        let gap = min_formality - avg;
        let extra = if gap >= 1.0 {
            8
        } else if gap >= 0.5 {
            4
        } else {
            0
        };
        let deduction = DEDUCT_FORMALITY_MISMATCH + extra;
        *score -= deduction;
        problems.push(RuleProblem {
            code: IssueCode::FormalitySituationMismatch,
            rule: "격식 수준".to_string(),
            deduction,
            detail: format!(
                "{}에 비해 코디가 너무 캐주얼합니다 (평균 격식도: {:.1})",
                situation, avg
            ),
        });
    } else if avg > max_formality {
        let gap = avg - max_formality;
        let extra = if gap >= 1.0 {
            8
        } else if gap >= 0.5 {
            4
        } else {
            0
        };
        let deduction = DEDUCT_FORMALITY_MISMATCH + extra;
        *score -= deduction;
        problems.push(RuleProblem {
            code: IssueCode::FormalitySituationMismatch,
            rule: "격식 수준".to_string(),
            deduction,
            detail: format!(
                "{}에 비해 코디가 너무 격식적입니다 (평균 격식도: {:.1})",
                situation, avg
            ),
        });
    }
}

/// Rule 12: 시즌 보정
fn rule_season(
    ctx: &OutfitContext,
    current_season: &str,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
) {
    let mut total = 0;
    let mut out_of_season = 0;
    let mut oos_names = Vec::new();

    for slot in &ctx.slots {
        if slot.seasons.is_empty() {
            continue;
        }
        total += 1;
        if !slot.seasons.iter().any(|s| s == current_season) {
            out_of_season += 1;
            oos_names.push(slot.clothing.name.as_str());
        }
    }

    if total == 0 {
        return;
    }

    let ratio = out_of_season as f32 / total as f32;
    if ratio >= 0.8 {
        *score -= DEDUCT_SEASON_ALL;
        problems.push(RuleProblem {
            code: IssueCode::SeasonalMismatch,
            rule: "시즌 보정".to_string(),
            deduction: DEDUCT_SEASON_ALL,
            detail: format!(
                "대부분의 아이템이 현재 계절({})에 맞지 않습니다: {}",
                current_season,
                oos_names.join(", ")
            ),
        });
    } else if ratio >= 0.4 {
        *score -= DEDUCT_SEASON_HALF;
        problems.push(RuleProblem {
            code: IssueCode::SeasonalMismatch,
            rule: "시즌 보정".to_string(),
            deduction: DEDUCT_SEASON_HALF,
            detail: format!(
                "일부 아이템이 현재 계절({})에 맞지 않습니다: {}",
                current_season,
                oos_names.join(", ")
            ),
        });
    }
}

// ─── 한 줄 총평 (규칙 기반, LLM 전에 결정론적으로 생성) ───

fn generate_summary(
    ctx: &OutfitContext,
    _verdict: &Verdict,
    problems: &[RuleProblem],
    strengths: &[OutfitStrength],
) -> String {
    // 아이템 이름 수집 (상의/아우터 중심)
    let outer_name = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Outer)
        .map(|s| s.clothing.name.as_str());
    let top_name = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Top)
        .map(|s| s.clothing.name.as_str());
    let bottom_name = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Bottom)
        .map(|s| s.clothing.name.as_str());

    let main_item = outer_name.or(top_name).unwrap_or("코디");

    if problems.is_empty() {
        // 문제 없음 → 강점 기반 총평
        let strength_hint = strengths.first().map(|s| s.detail.as_str()).unwrap_or("");
        return format!(
            "{}를 중심으로 잘 짜인 코디입니다. {}",
            main_item,
            if strength_hint.is_empty() {
                "밸런스가 좋습니다."
            } else {
                strength_hint
            }
        );
    }

    // 가장 큰 감점 문제를 기반으로 총평
    let worst = problems.iter().max_by_key(|p| p.deduction).unwrap();

    let issue_summary = match &worst.code {
        IssueCode::TooManyAccents => "포인트 아이템이 겹쳐 시선이 분산됩니다",
        IssueCode::LackOfStructure => "코디의 중심축이 약합니다",
        IssueCode::TooMuchNaturalTone => "웜톤으로만 구성되어 흐려 보입니다",
        IssueCode::LackOfContrast => "톤 대비가 부족해 밋밋합니다",
        IssueCode::TextureWorldConflict => "텍스처 세계관이 충돌합니다",
        IssueCode::StyleConflict => "스타일이 충돌합니다",
        IssueCode::StrongInner => "이너가 아우터와 경쟁합니다",
        IssueCode::BagConflict => "가방이 코디와 어울리지 않습니다",
        IssueCode::SeasonalMismatch => "계절에 맞지 않는 조합입니다",
        IssueCode::FormalitySituationMismatch => "상황과 격식 수준이 맞지 않습니다",
        IssueCode::SlotRoleMismatch => "아이템 배치가 어색합니다",
        IssueCode::WorldOvermatching => "밀리터리 아우터와 하의가 너무 비슷해 군복처럼 보입니다",
    };

    let fix_hint = if problems.len() == 1 {
        "하나만 바꾸면 훨씬 좋아집니다."
    } else if problems.len() <= 3 {
        "몇 가지 조정이 필요합니다."
    } else {
        "전체적인 재구성을 추천합니다."
    };

    let bottom_hint = bottom_name
        .map(|b| format!(" {}와의 조합에서", b))
        .unwrap_or_default();

    format!(
        "{}{} — {}. {}",
        main_item, bottom_hint, issue_summary, fix_hint
    )
}

// ─── 구조화된 suggestions 생성 ───

fn generate_structured_suggestions(problems: &[RuleProblem]) -> Vec<StructuredSuggestion> {
    problems
        .iter()
        .map(|p| match &p.code {
            IssueCode::TooManyAccents => StructuredSuggestion {
                suggestion_type: "replace_accent".to_string(),
                reason_code: p.code.clone(),
                reason: "포인트 아이템이 너무 많아 시선이 분산됩니다".to_string(),
                recommended_roles: vec!["베이스".to_string(), "연결템".to_string()],
                recommended_colors: vec![
                    "화이트".to_string(),
                    "그레이".to_string(),
                    "베이지".to_string(),
                ],
                recommended_examples: vec![
                    "화이트 크루넥 티셔츠".to_string(),
                    "그레이 스웻셔츠".to_string(),
                    "베이지 헨리넥".to_string(),
                ],
            },
            IssueCode::LackOfStructure => StructuredSuggestion {
                suggestion_type: "add_structure".to_string(),
                reason_code: p.code.clone(),
                reason: "코디의 중심축이 부족합니다".to_string(),
                recommended_roles: vec!["구조템".to_string(), "베이스".to_string()],
                recommended_colors: vec![
                    "인디고".to_string(),
                    "차콜".to_string(),
                    "블랙".to_string(),
                ],
                recommended_examples: vec![
                    "인디고 셀비지 데님".to_string(),
                    "차콜 울 팬츠".to_string(),
                    "블랙 코튼 치노".to_string(),
                ],
            },
            IssueCode::LackOfContrast => {
                if p.detail.contains("어두운") {
                    StructuredSuggestion {
                        suggestion_type: "replace_top".to_string(),
                        reason_code: p.code.clone(),
                        reason: "전체적으로 어두워 답답해 보입니다".to_string(),
                        recommended_roles: vec!["베이스".to_string()],
                        recommended_colors: vec![
                            "화이트".to_string(),
                            "크림".to_string(),
                            "라이트그레이".to_string(),
                        ],
                        recommended_examples: vec![
                            "화이트 크루넥 티셔츠".to_string(),
                            "크림 헨리넥".to_string(),
                        ],
                    }
                } else if p.detail.contains("밝은") {
                    StructuredSuggestion {
                        suggestion_type: "replace_bottom".to_string(),
                        reason_code: p.code.clone(),
                        reason: "전체적으로 밝아 흐려 보입니다".to_string(),
                        recommended_roles: vec!["베이스".to_string(), "구조템".to_string()],
                        recommended_colors: vec![
                            "인디고".to_string(),
                            "블랙".to_string(),
                            "차콜".to_string(),
                        ],
                        recommended_examples: vec![
                            "인디고 데님".to_string(),
                            "블랙 팬츠".to_string(),
                        ],
                    }
                } else {
                    StructuredSuggestion {
                        suggestion_type: "add_contrast".to_string(),
                        reason_code: p.code.clone(),
                        reason: "톤 대비가 부족해 밋밋합니다".to_string(),
                        recommended_roles: vec!["베이스".to_string(), "포인트".to_string()],
                        recommended_colors: vec![
                            "화이트".to_string(),
                            "블랙".to_string(),
                            "인디고".to_string(),
                        ],
                        recommended_examples: vec![
                            "밝거나 어두운 아이템으로 대비를 만들어보세요".to_string(),
                        ],
                    }
                }
            }
            IssueCode::TooMuchNaturalTone => StructuredSuggestion {
                suggestion_type: "replace_bottom".to_string(),
                reason_code: p.code.clone(),
                reason: "전체가 따뜻한 톤으로 몰려 있어 답답해 보여요".to_string(),
                recommended_roles: vec!["구조템".to_string(), "베이스".to_string()],
                recommended_colors: vec![
                    "차콜".to_string(),
                    "인디고".to_string(),
                    "블랙".to_string(),
                ],
                recommended_examples: vec![
                    "진그레이 바지".to_string(),
                    "인디고 데님".to_string(),
                    "블랙 코튼 트윌".to_string(),
                ],
            },
            IssueCode::TextureWorldConflict => StructuredSuggestion {
                suggestion_type: "replace_conflicting".to_string(),
                reason_code: p.code.clone(),
                reason: "텍스처 세계관이 충돌합니다".to_string(),
                recommended_roles: vec!["베이스".to_string(), "연결템".to_string()],
                recommended_colors: vec![],
                recommended_examples: vec![
                    "같은 텍스처 계열의 아이템으로 교체해보세요".to_string(),
                ],
            },
            IssueCode::StyleConflict => StructuredSuggestion {
                suggestion_type: "replace_style".to_string(),
                reason_code: p.code.clone(),
                reason: "스타일이 충돌합니다".to_string(),
                recommended_roles: vec!["베이스".to_string()],
                recommended_colors: vec![],
                recommended_examples: vec![
                    "충돌하는 아이템 하나를 베이직으로 교체해보세요".to_string(),
                ],
            },
            IssueCode::StrongInner => StructuredSuggestion {
                suggestion_type: "replace_top".to_string(),
                reason_code: p.code.clone(),
                reason: "이너가 너무 강해서 아우터와 경쟁합니다".to_string(),
                recommended_roles: vec!["베이스".to_string(), "연결템".to_string()],
                recommended_colors: vec![
                    "화이트".to_string(),
                    "그레이".to_string(),
                    "크림".to_string(),
                ],
                recommended_examples: vec![
                    "화이트 크루넥 티셔츠".to_string(),
                    "그레이 스웻셔츠".to_string(),
                ],
            },
            IssueCode::BagConflict => StructuredSuggestion {
                suggestion_type: "replace_bag".to_string(),
                reason_code: p.code.clone(),
                reason: "가방이 코디와 어울리지 않습니다".to_string(),
                recommended_roles: vec!["연결템".to_string(), "구조템".to_string()],
                recommended_colors: vec![
                    "블랙".to_string(),
                    "네이비".to_string(),
                    "카키".to_string(),
                ],
                recommended_examples: vec![
                    "블랙 캔버스 토트".to_string(),
                    "네이비 나일론 백팩".to_string(),
                ],
            },
            IssueCode::SeasonalMismatch => StructuredSuggestion {
                suggestion_type: "replace_seasonal".to_string(),
                reason_code: p.code.clone(),
                reason: "계절에 맞지 않는 아이템이 있습니다".to_string(),
                recommended_roles: vec![],
                recommended_colors: vec![],
                recommended_examples: vec![
                    "현재 계절에 맞는 두께와 소재로 교체해보세요".to_string(),
                ],
            },
            IssueCode::FormalitySituationMismatch => {
                if p.detail.contains("캐주얼합니다") {
                    StructuredSuggestion {
                        suggestion_type: "upgrade_formality".to_string(),
                        reason_code: p.code.clone(),
                        reason: "상황에 비해 너무 캐주얼합니다".to_string(),
                        recommended_roles: vec!["구조템".to_string()],
                        recommended_colors: vec!["네이비".to_string(), "차콜".to_string()],
                        recommended_examples: vec![
                            "옥스포드 셔츠".to_string(),
                            "테일러드 자켓".to_string(),
                            "치노 팬츠".to_string(),
                        ],
                    }
                } else {
                    StructuredSuggestion {
                        suggestion_type: "downgrade_formality".to_string(),
                        reason_code: p.code.clone(),
                        reason: "상황에 비해 너무 격식적입니다".to_string(),
                        recommended_roles: vec!["베이스".to_string(), "연결템".to_string()],
                        recommended_colors: vec![],
                        recommended_examples: vec![
                            "스웻셔츠".to_string(),
                            "스니커".to_string(),
                            "캔버스 토트".to_string(),
                        ],
                    }
                }
            }
            IssueCode::SlotRoleMismatch => StructuredSuggestion {
                suggestion_type: "replace_slot".to_string(),
                reason_code: p.code.clone(),
                reason: p.detail.clone(),
                recommended_roles: vec![],
                recommended_colors: vec![],
                recommended_examples: vec![
                    "해당 자리에 맞는 역할의 아이템으로 교체해보세요".to_string(),
                ],
            },
            IssueCode::WorldOvermatching => StructuredSuggestion {
                suggestion_type: "replace_bottom".to_string(),
                reason_code: p.code.clone(),
                reason: "밀리터리 아우터와 자연톤 하의가 너무 비슷해 군복처럼 보입니다".to_string(),
                recommended_roles: vec!["베이스".to_string(), "구조템".to_string()],
                recommended_colors: vec![
                    "인디고".to_string(),
                    "차콜".to_string(),
                    "블랙".to_string(),
                ],
                recommended_examples: vec![
                    "인디고 셀비지 데님".to_string(),
                    "차콜 울 팬츠".to_string(),
                    "블랙 코튼 치노".to_string(),
                ],
            },
        })
        .collect()
}

// ─── LLM 설명용 포맷터 ───

pub fn format_items_description(slots: &[OutfitSlot]) -> String {
    slots
        .iter()
        .map(|s| {
            let tw = if s.texture_worlds.is_empty() {
                "N/A".to_string()
            } else {
                s.texture_worlds.join(", ")
            };
            format!(
                "- {}: {} (색상: {}, 톤: {}, 색온도: {}, 역할: {}, 스타일: {}, 텍스처: {}, 격식: {}, 존재감: {})",
                s.slot.label(),
                s.clothing.name,
                s.clothing.color.as_deref().unwrap_or("N/A"),
                s.clothing.tone.map(|v| v.as_str()).unwrap_or("N/A"),
                s.clothing.color_temperature.as_deref().unwrap_or("N/A"),
                s.clothing.role.map(|v| v.as_str()).unwrap_or("N/A"),
                s.clothing.style.map(|v| v.as_str()).unwrap_or("N/A"),
                tw,
                s.clothing.formality_level.map(|l| l.to_string()).unwrap_or("N/A".to_string()),
                s.clothing.statement_level.map(|l| l.to_string()).unwrap_or("N/A".to_string()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_problems_description(problems: &[RuleProblem]) -> String {
    if problems.is_empty() {
        return "없음 (완벽한 코디!)".to_string();
    }
    problems
        .iter()
        .map(|p| format!("- [{}] {} (-{}점)", p.rule, p.detail, p.deduction))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_strengths_description(strengths: &[OutfitStrength]) -> String {
    if strengths.is_empty() {
        return "".to_string();
    }
    strengths
        .iter()
        .map(|s| format!("- [{}] {}", s.rule, s.detail))
        .collect::<Vec<_>>()
        .join("\n")
}
