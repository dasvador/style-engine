//! 무드가 기존 규칙의 해석을 바꾸는 곳. 지금은 글램만.
//!
//! 운영 엔진(`style_engine::evaluate`)은 "무난한 일상복" 을 기준으로 쓰였다. 글램을
//! 고른 사용자에게 그 기준을 그대로 대면, 시선을 끄는 옷을 쓴 조합이 같은 이유로 여러 번
//! 깎여 기본 티셔츠 조합에 밀린다(2026-10-17 실제 요청 12회 중 강한 상의가 오늘의 추천에
//! 오른 것은 0회). 여기서는 글램에서 근거가 맞지 않는 감점만 되돌린다. 가산점은 새로 주지
//! 않고, 되돌린 뒤에도 운영 상한(95)을 넘지 않는다.
//!
//! 되돌리는 것 (전부 저장된 속성으로만 판단한다):
//! 1. 조용한 아우터 안의 강한 상의 — 아우터가 구조·연결·베이스면 이너와 경쟁할 상대가
//!    없다. 아우터도 포인트·약한포인트면 그대로 깎는다.
//! 2. 보조 아이템만 더한 포인트 과다 — 옷의 포인트가 하나이고 넘치는 몫이 가방·신발뿐이면
//!    "포인트 과다" 와 가방의 "슬롯 역할" 을 지운다. 같은 시각 문제를 가방 규칙
//!    (`rule_bag` 의 "가방까지 포인트")이 이미 한 번 깎고 있어 셋이 겹쳤다. 옷 두 곳이
//!    포인트면 실제로 경쟁하므로 그대로다.
//! 3. 하의가 유일한 포인트일 때의 슬롯 역할 감점 — 특징적인 스커트가 하의라는 이유로 밀리지
//!    않게 한다.
//! 4. 홈의 '일상' 격식 상한 — 홈은 상황을 묻지 않고 '일상' 을 보낸다. 글램에서는 상한을
//!    2.5 에서 3.5 로 본다. 사용자가 고른 출근·비즈니스는 손대지 않는다.
//! 5. 올블랙 감점 — 옷에 글램 요소가 있고, 옷끼리 광택과 무광이 섞이거나 형태 디테일
//!    (원숄더·드레이프·비대칭)이 있으면 어두운 색만으로 답답하다고 보지 않는다.
//!
//! 겨울 코트는 착장의 일부로 함께 채점한다. 한겨울 글램은 실내 착장 위에 두꺼운 코트를
//! 걸치고 이동하는 장면이라 안의 착장이 가벼워도 되지만(보온은 코트를 포함해 판정한다),
//! 코트가 주인공인 글램도 있어서 코트를 착장 밖으로 빼지 않는다. 퍼 코트와 강한 상의처럼
//! 옷 두 곳이 강하면 "착장마다 중심 소재 하나" 기준으로 계속 깎는다(2026-10-17 사용자).
//!
//! 날씨·계절·스타일 충돌·텍스처 충돌 감점은 건드리지 않는다.

use crate::models::clothing::Clothing;
use crate::models::outfit::{EvaluationResult, IssueCode, OutfitContext, OutfitStrength, SlotKind};
use crate::models::style_vocab::{Role, StyleGenre};
use crate::services::style_engine;

/// 글램의 '일상' 격식 상한.
const GLAM_DAILY_MAX_FORMALITY: f32 = 3.5;

/// 무드를 반영한 평가. 글램이 아니면 운영 평가 그대로다.
pub fn evaluate_for_mood(
    ctx: &OutfitContext,
    current_season: Option<&str>,
    mood: Option<StyleGenre>,
) -> EvaluationResult {
    let mut e = style_engine::evaluate(ctx, current_season);
    if mood == Some(StyleGenre::Glam) {
        apply_glam(ctx, &mut e);
    }
    e
}

fn is_garment(kind: SlotKind) -> bool {
    matches!(kind, SlotKind::Top | SlotKind::Bottom | SlotKind::Outer)
}

/// 옷의 표면. 광택(새틴·메탈릭·광택 가죽) · 결(퍼·시어링·벨벳) · 무광으로 나눈다.
/// 메탈릭은 소재값이 아니라 설명(`texture_keywords`)으로 들어온다. 설명에 "matte" 가
/// 있는 가죽은 무광이다.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Surface {
    Sheen,
    Pile,
    Matte,
}

fn surface(c: &Clothing) -> Surface {
    let m = c.material_primary.as_deref().unwrap_or("");
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    if matches!(m, "faux fur" | "shearling" | "velvet")
        || ["faux fur", "shearling", "velvet"]
            .iter()
            .any(|w| k.contains(w))
    {
        return Surface::Pile;
    }
    if m == "satin"
        || (m == "leather" && !k.contains("matte"))
        || ["metallic", "lurex", "sheen", "lustrous"]
            .iter()
            .any(|w| k.contains(w))
    {
        return Surface::Sheen;
    }
    Surface::Matte
}

/// 형태로 시선을 모으는 옷인가.
fn has_sculpted_line(c: &Clothing) -> bool {
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    ["one-shoulder", "draped", "cowl", "asymmetric", "hourglass"]
        .iter()
        .any(|w| k.contains(w))
}

/// 어두운 색 위주여도 소재·형태로 특징이 드러나는가(규칙 5). 옷(상의·하의·아우터)에
/// 글램 요소가 있고, 광택과 무광이 섞이거나 형태 디테일이 있어야 한다. 상담의 평가 도구도
/// 같은 판단을 쓴다.
pub fn dark_look_has_contrast(garments: &[&Clothing]) -> bool {
    let glam_garment = garments
        .iter()
        .any(|c| crate::services::mood_gate::is_glam_piece(c));
    let surfaces: std::collections::HashSet<Surface> =
        garments.iter().map(|c| surface(c)).collect();
    let surface_mix = surfaces.len() >= 2;
    let sculpted = garments.iter().any(|c| has_sculpted_line(c));
    glam_garment && (surface_mix || sculpted)
}

fn daily_formality_waiver(ctx: &OutfitContext, deduction: i32) -> i32 {
    let levels: Vec<f32> = ctx
        .slots
        .iter()
        .filter_map(|s| s.clothing.formality_level)
        .map(f32::from)
        .collect();
    if levels.is_empty() {
        return 0;
    }
    let avg = levels.iter().sum::<f32>() / levels.len() as f32;
    // 운영과 같은 계단(12 + 격차 0.5 이상 4, 1.0 이상 8)으로 넓힌 상한에서 다시 잰다.
    let kept = if avg > GLAM_DAILY_MAX_FORMALITY {
        let gap = avg - GLAM_DAILY_MAX_FORMALITY;
        12 + if gap >= 1.0 {
            8
        } else if gap >= 0.5 {
            4
        } else {
            0
        }
    } else {
        0
    };
    (deduction - kept).max(0)
}

fn apply_glam(ctx: &OutfitContext, e: &mut EvaluationResult) {
    let slot = |k: SlotKind| ctx.slots.iter().find(|s| s.slot == k);
    let garment_accents: Vec<&Clothing> = ctx
        .slots
        .iter()
        .filter(|s| is_garment(s.slot) && s.clothing.role == Some(Role::Accent))
        .map(|s| &s.clothing)
        .collect();
    let outer_quiet = slot(SlotKind::Outer).is_some_and(|o| {
        matches!(
            o.clothing.role,
            Some(Role::Structural | Role::Connector | Role::Base)
        )
    });
    let top = slot(SlotKind::Top);
    let bottom = slot(SlotKind::Bottom);
    let accessory_names: Vec<&str> = ctx
        .slots
        .iter()
        .filter(|s| !is_garment(s.slot))
        .map(|s| s.clothing.name.as_str())
        .collect();
    let garments: Vec<&Clothing> = ctx
        .slots
        .iter()
        .filter(|s| is_garment(s.slot))
        .map(|s| &s.clothing)
        .collect();
    let dark_contrast = dark_look_has_contrast(&garments);
    let daily = matches!(ctx.situation.as_deref(), Some("일상" | "캐주얼") | None);

    let mut waived = 0;
    let mut notes: Vec<String> = Vec::new();
    let mut keep = Vec::with_capacity(e.problems.len());
    for p in e.problems.drain(..) {
        let about = |name: &str| p.detail.contains(name);
        let give_back = match p.code {
            // 1. 조용한 아우터 안의 강한 상의
            IssueCode::StrongInner if outer_quiet => {
                notes.push("조용한 아우터가 상의의 포인트를 받쳐 줍니다".into());
                p.deduction
            }
            IssueCode::SlotRoleMismatch
                if outer_quiet
                    && top.is_some_and(|t| {
                        t.clothing.role == Some(Role::Accent) && about(&t.clothing.name)
                    }) =>
            {
                p.deduction
            }
            // 2. 넘치는 포인트가 가방·신발뿐
            IssueCode::TooManyAccents if garment_accents.len() <= 1 => {
                notes.push("포인트는 옷 한 곳이고 가방은 작은 보조 포인트입니다".into());
                p.deduction
            }
            IssueCode::SlotRoleMismatch
                if garment_accents.len() <= 1 && accessory_names.iter().any(|n| about(n)) =>
            {
                p.deduction
            }
            // 3. 하의가 유일한 포인트
            IssueCode::SlotRoleMismatch
                if garment_accents.len() == 1
                    && bottom.is_some_and(|b| {
                        b.clothing.role == Some(Role::Accent) && about(&b.clothing.name)
                    }) =>
            {
                notes.push("하의가 코디의 중심입니다".into());
                p.deduction
            }
            // 4. 홈 '일상' 의 격식 상한
            IssueCode::FormalitySituationMismatch if daily && p.detail.contains("격식적") => {
                let back = daily_formality_waiver(ctx, p.deduction);
                if back < p.deduction {
                    // 넓힌 상한도 넘으면 남은 몫은 그대로 깎는다.
                    keep.push(crate::models::outfit::RuleProblem {
                        deduction: p.deduction - back,
                        ..p
                    });
                    waived += back;
                    continue;
                }
                back
            }
            // 5. 소재·형태 차이가 있는 올블랙
            IssueCode::LackOfContrast
                if p.rule == "밝기 밸런스" && p.detail.contains("어두운") && dark_contrast =>
            {
                notes.push("어두운 색 안에서 소재와 형태로 대비를 만듭니다".into());
                p.deduction
            }
            _ => 0,
        };
        if give_back > 0 {
            waived += give_back;
        } else {
            keep.push(p);
        }
    }
    e.problems = keep;
    if waived > 0 {
        e.score = (e.score + waived).min(style_engine::SCORE_CEILING);
        notes.dedup();
        for n in notes {
            e.strengths.push(OutfitStrength {
                rule: "글램".into(),
                detail: n,
            });
        }
        e.verdict = crate::models::outfit::Verdict::from_score(e.score);
        e.suggestions = style_engine::generate_structured_suggestions(&e.problems);
        e.summary = style_engine::generate_summary(ctx, &e.verdict, &e.problems, &e.strengths);
    }
}
