use crate::models::outfit::{
    EvaluationResult, IssueCode, OutfitContext, OutfitSlot, OutfitStrength, RuleProblem, SlotKind,
    Verdict,
};

const DEDUCT_BANCHAN_OVERLOAD: i32 = 20;
const DEDUCT_NO_BAB: i32 = 10;
const DEDUCT_ALL_DARK: i32 = 15;
const DEDUCT_ALL_BRIGHT: i32 = 10;
const DEDUCT_STYLE_CONFLICT: i32 = 20;
const DEDUCT_INNER_ISSUE: i32 = 10;
const DEDUCT_BAG_ISSUE: i32 = 10;
const DEDUCT_SEASON_ALL: i32 = 15;
const DEDUCT_SEASON_HALF: i32 = 5;
const DEDUCT_LACK_OF_CONTRAST: i32 = 15;
const DEDUCT_NATURAL_TONE_OVERLOAD: i32 = 12;
const DEDUCT_TEXTURE_WORLD_CONFLICT: i32 = 15;
const DEDUCT_FORMALITY_MISMATCH: i32 = 12;

pub fn evaluate(ctx: &OutfitContext, current_season: Option<&str>) -> EvaluationResult {
    let mut score = 100i32;
    let mut problems = Vec::new();
    let mut strengths = Vec::new();

    rule_bab_banchan(ctx, &mut score, &mut problems, &mut strengths);
    rule_brightness(ctx, &mut score, &mut problems, &mut strengths);
    rule_lack_of_contrast(ctx, &mut score, &mut problems, &mut strengths);
    rule_natural_tone(ctx, &mut score, &mut problems);
    rule_style_conflict(ctx, &mut score, &mut problems);
    rule_texture_world_conflict(ctx, &mut score, &mut problems, &mut strengths);
    rule_inner(ctx, &mut score, &mut problems);
    rule_bag(ctx, &mut score, &mut problems);
    rule_formality_situation(ctx, &mut score, &mut problems);
    if let Some(season) = current_season {
        rule_season(ctx, season, &mut score, &mut problems);
    }

    score = score.max(0);

    let suggestions = generate_suggestions(&problems);
    let verdict = Verdict::from_score(score);

    EvaluationResult {
        score,
        verdict,
        problems,
        strengths,
        suggestions,
    }
}

/// Rule 1: 밥/반찬 밸런스
fn rule_bab_banchan(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let mut banchan_count = 0;
    let mut bab_count = 0;
    let mut weak_banchan_count = 0;
    let mut banchan_names = Vec::new();

    for slot in &ctx.slots {
        match slot.clothing.role.as_deref() {
            Some("반찬") => {
                banchan_count += 1;
                banchan_names.push(slot.clothing.name.as_str());
            }
            Some("밥") => bab_count += 1,
            Some("약한반찬") => weak_banchan_count += 1,
            _ => {}
        }
    }

    if banchan_count >= 2 {
        *score -= DEDUCT_BANCHAN_OVERLOAD;
        problems.push(RuleProblem {
            code: IssueCode::TooManyAccents,
            rule: "밥/반찬 밸런스".to_string(),
            deduction: DEDUCT_BANCHAN_OVERLOAD,
            detail: format!(
                "반찬(포인트) 아이템이 {}개로 과합니다: {}",
                banchan_count,
                banchan_names.join(", ")
            ),
        });
    }

    if bab_count == 0 && !ctx.slots.is_empty() {
        *score -= DEDUCT_NO_BAB;
        problems.push(RuleProblem {
            code: IssueCode::LackOfStructure,
            rule: "밥/반찬 밸런스".to_string(),
            deduction: DEDUCT_NO_BAB,
            detail: "밥(베이스) 아이템이 없어 코디의 중심이 부족합니다".to_string(),
        });
    }

    // 좋은 밸런스 감지
    if bab_count >= 1 && banchan_count == 1 && problems.is_empty() {
        strengths.push(OutfitStrength {
            rule: "밥/반찬 밸런스".to_string(),
            detail: "밥과 반찬의 비율이 적절합니다. 포인트 아이템이 잘 살아납니다".to_string(),
        });
    }

    if bab_count >= 1 && banchan_count == 0 && weak_banchan_count >= 1 {
        strengths.push(OutfitStrength {
            rule: "밥/반찬 밸런스".to_string(),
            detail: "은은한 포인트로 차분하면서도 개성 있는 조합입니다".to_string(),
        });
    }
}

/// Rule 2: 밝기 밸런스
fn rule_brightness(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let tones: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.tone.as_deref())
        .collect();

    if tones.len() < 2 {
        return;
    }

    let all_dark = tones.iter().all(|t| *t == "어두움");
    let all_bright = tones.iter().all(|t| *t == "밝음");

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
        let has_dark = tones.iter().any(|t| *t == "어두움");
        let has_bright = tones.iter().any(|t| *t == "밝음");
        if has_dark && has_bright {
            strengths.push(OutfitStrength {
                rule: "밝기 밸런스".to_string(),
                detail: "밝음과 어두움의 대비가 잘 잡혀있어 시각적으로 깔끔합니다".to_string(),
            });
        }
    }
}

/// Rule 3: 대비 부족 (톤 + 채도가 모두 유사)
fn rule_lack_of_contrast(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
    strengths: &mut Vec<OutfitStrength>,
) {
    let items: Vec<(&str, &str)> = ctx
        .slots
        .iter()
        .filter_map(|s| {
            let tone = s.clothing.tone.as_deref()?;
            let sat = s.clothing.saturation.as_deref()?;
            Some((tone, sat))
        })
        .collect();

    if items.len() < 2 {
        return;
    }

    let first = items[0];
    let all_same = items.iter().all(|i| *i == first);

    if all_same && first.0 == "중간" && first.1 == "중간" {
        *score -= DEDUCT_LACK_OF_CONTRAST;
        problems.push(RuleProblem {
            code: IssueCode::LackOfContrast,
            rule: "대비 부족".to_string(),
            deduction: DEDUCT_LACK_OF_CONTRAST,
            detail: "톤과 채도가 모두 중간이라 밋밋해 보일 수 있습니다. 밝거나 어두운 아이템으로 대비를 만들어보세요".to_string(),
        });
    }

    // 다양한 톤 조합이면 강점
    let unique_tones: std::collections::HashSet<&str> =
        items.iter().map(|(t, _)| *t).collect();
    if unique_tones.len() >= 3 {
        strengths.push(OutfitStrength {
            rule: "대비".to_string(),
            detail: "다양한 톤이 조화롭게 구성되어 입체감이 있습니다".to_string(),
        });
    }
}

/// Rule 4: 자연톤 과다 (올리브 + 베이지 + 러스트 등 warm 톤만)
fn rule_natural_tone(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
) {
    let temps: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.color_temperature.as_deref())
        .collect();

    if temps.len() < 3 {
        return;
    }

    let warm_count = temps.iter().filter(|t| **t == "warm").count();
    if warm_count == temps.len() {
        *score -= DEDUCT_NATURAL_TONE_OVERLOAD;
        problems.push(RuleProblem {
            code: IssueCode::TooMuchNaturalTone,
            rule: "자연톤 과다".to_string(),
            deduction: DEDUCT_NATURAL_TONE_OVERLOAD,
            detail: "모든 아이템이 웜톤(자연색)이라 전체적으로 흐려 보일 수 있습니다. 쿨톤(네이비, 그레이) 아이템을 하나 넣어보세요".to_string(),
        });
    }
}

/// Rule 5: 스타일 충돌
fn rule_style_conflict(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let styles: Vec<&str> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.style.as_deref())
        .filter(|s| *s != "베이직")
        .collect();

    let conflicts = [("포멀", "스포츠"), ("밀리터리", "포멀")];

    for (a, b) in &conflicts {
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

    let strong_styles = ["워크", "밀리터리"];
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

/// Rule 6: 텍스처 월드 충돌
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

    let conflicts = [("tailoring", "sweat"), ("outdoor", "tailoring")];

    for (a, b) in &conflicts {
        let has_a = all_worlds.iter().any(|w| w == a);
        let has_b = all_worlds.iter().any(|w| w == b);
        if has_a && has_b {
            *score -= DEDUCT_TEXTURE_WORLD_CONFLICT;
            problems.push(RuleProblem {
                code: IssueCode::TextureWorldConflict,
                rule: "텍스처 충돌".to_string(),
                deduction: DEDUCT_TEXTURE_WORLD_CONFLICT,
                detail: format!(
                    "{} + {} 텍스처가 섞여 어색할 수 있습니다",
                    a, b
                ),
            });
            return;
        }
    }

    // 워크웨어+밀리터리 조합은 강점
    let has_workwear = all_worlds.iter().any(|w| *w == "workwear");
    let has_military = all_worlds.iter().any(|w| *w == "military");
    if has_workwear && has_military {
        strengths.push(OutfitStrength {
            rule: "텍스처 조화".to_string(),
            detail: "워크웨어와 밀리터리가 자연스럽게 어우러지는 아메카지 스타일입니다".to_string(),
        });
    }
}

/// Rule 7: 이너 규칙
fn rule_inner(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let has_outer = ctx.slots.iter().any(|s| s.slot == SlotKind::Outer);
    if !has_outer {
        return;
    }

    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    if let Some(top_slot) = top {
        let is_strong_inner = top_slot.clothing.role.as_deref() == Some("반찬")
            || (top_slot.clothing.tone.as_deref() == Some("어두움")
                && top_slot.clothing.saturation.as_deref() == Some("높음"));

        if is_strong_inner {
            *score -= DEDUCT_INNER_ISSUE;
            problems.push(RuleProblem {
                code: IssueCode::StrongInner,
                rule: "이너 규칙".to_string(),
                deduction: DEDUCT_INNER_ISSUE,
                detail: format!(
                    "이너({})가 너무 강해서 아우터와 경쟁합니다. 밝거나 중립적인 이너를 추천합니다",
                    top_slot.clothing.name
                ),
            });
        }
    }
}

/// Rule 8: 가방 규칙
fn rule_bag(ctx: &OutfitContext, score: &mut i32, problems: &mut Vec<RuleProblem>) {
    let bag = ctx.slots.iter().find(|s| s.slot == SlotKind::Bag);
    let bag_slot = match bag {
        Some(b) => b,
        None => return,
    };

    if bag_slot.clothing.role.as_deref() == Some("반찬") {
        *score -= DEDUCT_BAG_ISSUE;
        problems.push(RuleProblem {
            code: IssueCode::BagConflict,
            rule: "가방 규칙".to_string(),
            deduction: DEDUCT_BAG_ISSUE,
            detail: format!(
                "가방({})이 너무 눈에 띕니다. 가방은 코디를 정리하는 역할이 좋습니다",
                bag_slot.clothing.name
            ),
        });
        return;
    }

    if let Some(bag_style) = bag_slot.clothing.style.as_deref() {
        let dominant_styles: Vec<&str> = ctx
            .slots
            .iter()
            .filter(|s| s.slot != SlotKind::Bag)
            .filter_map(|s| s.clothing.style.as_deref())
            .filter(|s| *s != "베이직")
            .collect();

        if !dominant_styles.is_empty()
            && bag_style != "베이직"
            && !dominant_styles.contains(&bag_style)
        {
            let dominant = dominant_styles.first().unwrap_or(&"");
            if (*dominant == "밀리터리" && bag_style == "포멀")
                || (*dominant == "포멀" && bag_style == "스포츠")
            {
                *score -= DEDUCT_BAG_ISSUE;
                problems.push(RuleProblem {
                    code: IssueCode::BagConflict,
                    rule: "가방 규칙".to_string(),
                    deduction: DEDUCT_BAG_ISSUE,
                    detail: format!(
                        "가방({}, {})이 전체 코디({})와 어울리지 않습니다",
                        bag_slot.clothing.name, bag_style, dominant
                    ),
                });
            }
        }
    }
}

/// Rule 9: 격식 수준 vs 상황 미스매치
fn rule_formality_situation(
    ctx: &OutfitContext,
    score: &mut i32,
    problems: &mut Vec<RuleProblem>,
) {
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
        *score -= DEDUCT_FORMALITY_MISMATCH;
        problems.push(RuleProblem {
            code: IssueCode::FormalitySituationMismatch,
            rule: "격식 수준".to_string(),
            deduction: DEDUCT_FORMALITY_MISMATCH,
            detail: format!(
                "{}에 비해 코디가 너무 캐주얼합니다 (평균 격식도: {:.1})",
                situation, avg
            ),
        });
    } else if avg > max_formality {
        *score -= DEDUCT_FORMALITY_MISMATCH;
        problems.push(RuleProblem {
            code: IssueCode::FormalitySituationMismatch,
            rule: "격식 수준".to_string(),
            deduction: DEDUCT_FORMALITY_MISMATCH,
            detail: format!(
                "{}에 비해 코디가 너무 격식적입니다 (평균 격식도: {:.1})",
                situation, avg
            ),
        });
    }
}

/// Rule 10: 시즌 보정
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

fn generate_suggestions(problems: &[RuleProblem]) -> Vec<String> {
    problems
        .iter()
        .map(|p| match &p.code {
            IssueCode::TooManyAccents => {
                "반찬 아이템을 하나 줄이고, 밥(베이스) 아이템으로 교체해보세요".to_string()
            }
            IssueCode::LackOfStructure => {
                "화이트 티, 그레이 팬츠 등 베이스 아이템을 추가해보세요".to_string()
            }
            IssueCode::LackOfContrast => {
                if p.detail.contains("어두운") {
                    "밝은 이너(화이트, 크림)를 추가하면 균형이 잡힙니다".to_string()
                } else if p.detail.contains("밝은") {
                    "어두운 하의(인디고 데님, 블랙 팬츠)로 무게감을 잡아보세요".to_string()
                } else {
                    "밝거나 어두운 아이템을 하나 넣어 대비를 만들어보세요".to_string()
                }
            }
            IssueCode::TooMuchNaturalTone => {
                "쿨톤 아이템(네이비, 그레이, 블랙)을 하나 넣어 균형을 맞춰보세요".to_string()
            }
            IssueCode::TextureWorldConflict => {
                "텍스처가 충돌하는 아이템 하나를 같은 계열로 변경해보세요".to_string()
            }
            IssueCode::StyleConflict => {
                "스타일이 겹치는 아이템 하나를 베이직 아이템으로 바꿔보세요".to_string()
            }
            IssueCode::StrongInner => {
                "이너를 화이트, 그레이 등 중립적인 아이템으로 변경해보세요".to_string()
            }
            IssueCode::BagConflict => {
                "가방을 블랙, 네이비 등 차분한 색상/스타일로 변경해보세요".to_string()
            }
            IssueCode::SeasonalMismatch => {
                "현재 계절에 맞는 두께와 소재의 아이템으로 교체해보세요".to_string()
            }
            IssueCode::FormalitySituationMismatch => {
                if p.detail.contains("캐주얼합니다") {
                    "상황에 맞게 격식도를 올려보세요 (셔츠, 테일러드 자켓 등)".to_string()
                } else {
                    "상황에 맞게 캐주얼하게 풀어보세요 (스웻셔츠, 스니커 등)".to_string()
                }
            }
        })
        .collect()
}

/// Format outfit items into a human-readable description for LLM
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
                s.clothing.tone.as_deref().unwrap_or("N/A"),
                s.clothing.color_temperature.as_deref().unwrap_or("N/A"),
                s.clothing.role.as_deref().unwrap_or("N/A"),
                s.clothing.style.as_deref().unwrap_or("N/A"),
                tw,
                s.clothing.formality_level.map(|l| l.to_string()).unwrap_or("N/A".to_string()),
                s.clothing.statement_level.map(|l| l.to_string()).unwrap_or("N/A".to_string()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format problems into a description for LLM
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

/// Format strengths into a description for LLM
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
