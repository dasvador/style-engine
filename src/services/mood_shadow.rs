//! shadow 실험: 기존 규칙이 잘 어울리는 조합을 부당하게 깎는가. 글램 밖의 무드만 잰다.
//!
//! 운영 점수는 바꾸지 않는다. 같은 후보·날씨·취향·이력으로 낸 운영 점수에 아래 두
//! 보정을 더한 값을 따로 계산해 로그로 남긴다(`recommendation::multi` 의
//! `mood_shadow=` 줄). 가산점을 새로 주는 것이 아니라, 감점 중 근거가 약한 것을 되돌린다.
//!
//! S1(글램 '일상' 격식 범위)은 2026-10-17 글램 규칙(`services::mood_rules`)으로 옮겼다.
//!
//! S2 — 아우터 안의 강한 상의. 운영은 아우터가 있으면 포인트 상의를 무조건 깎는다
//!   (이너 규칙 −10, 슬롯 역할 −8). 2026-04 첫 엔진에서 온 규칙으로, 근거는 "이너가
//!   아우터와 경쟁한다" 이다. 그런데 아우터가 구조템·연결템·베이스처럼 조용하면 경쟁할
//!   상대가 없다. 아우터도 포인트·약한포인트일 때만 깎는다. 무드와 무관하게 적용한다 —
//!   규칙의 근거가 무드와 무관하기 때문이다.
//!
//! 날씨·보온 판정, 포인트 2개 이상 감점, 가방 감점은 그대로다. 되돌린 점수도 운영과
//! 같은 상한(95)에서 자른다.

use crate::models::outfit::{EvaluationResult, IssueCode, OutfitContext, SlotKind};
use crate::models::style_vocab::{Role, StyleGenre};

/// 보정이 운영 점수에 더할 값. 0 이상이다 — 감점을 되돌리기만 한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Waiver {
    pub inner: i32,
}

impl Waiver {
    pub fn total(&self) -> i32 {
        self.inner
    }
}

/// 운영 평가(`style_engine::evaluate`)의 감점 중 S1·S2 가 되돌릴 몫.
pub fn waiver(ctx: &OutfitContext, eval: &EvaluationResult, _mood: Option<StyleGenre>) -> Waiver {
    let mut w = Waiver::default();

    // S2: 아우터가 조용하면 "이너가 아우터와 경쟁" 감점을 되돌린다.
    let outer_role = ctx
        .slots
        .iter()
        .find(|s| s.slot == SlotKind::Outer)
        .and_then(|s| s.clothing.role);
    let outer_is_quiet = matches!(
        outer_role,
        Some(Role::Structural | Role::Connector | Role::Base)
    );
    let top = ctx.slots.iter().find(|s| s.slot == SlotKind::Top);
    if outer_is_quiet
        && let Some(top) = top
        && top.clothing.role == Some(Role::Accent)
    {
        for p in &eval.problems {
            let about_top = p.detail.contains(&top.clothing.name);
            match p.code {
                IssueCode::StrongInner if about_top => w.inner += p.deduction,
                IssueCode::SlotRoleMismatch if about_top => w.inner += p.deduction,
                _ => {}
            }
        }
    }
    // 되돌린 뒤에도 운영 점수의 상한(95)을 넘지 않는다. 운영은 감점과 가점을 더하면서
    // 상한에서 자르므로, 감점을 그대로 더해 주면 감점이 없던 조합보다 높아진다 — 그건
    // 부당한 감점을 지우는 것이 아니라 가산점이다.
    let excess = eval.score + w.total() - crate::services::style_engine::SCORE_CEILING;
    if excess > 0 {
        w.inner -= excess.min(w.inner);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::clothing::Clothing;
    use crate::models::outfit::OutfitSlot;
    use crate::models::style_vocab::Thickness;
    use crate::services::style_engine;

    fn slot(kind: SlotKind, name: &str, role: Role, formality: i8) -> OutfitSlot {
        let now = chrono::NaiveDateTime::default();
        OutfitSlot {
            slot: kind,
            clothing: Clothing {
                id: name.into(),
                name: name.into(),
                category: match kind {
                    SlotKind::Top => "상의",
                    SlotKind::Bottom => "하의",
                    SlotKind::Outer => "아우터",
                    SlotKind::Shoes => "신발",
                    SlotKind::Bag => "가방",
                }
                .into(),
                color: None,
                thickness: Thickness::Medium,
                image_url: None,
                tone: None,
                saturation: None,
                style: None,
                weight: None,
                role: Some(role),
                color_temperature: None,
                versatility: None,
                statement_level: None,
                formality_level: Some(formality),
                gender: None,
                style_mood: None,
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
                created_at: now,
                updated_at: now,
            },
            seasons: Vec::new(),
            texture_worlds: Vec::new(),
        }
    }

    fn ctx(slots: Vec<OutfitSlot>) -> OutfitContext {
        OutfitContext {
            slots,
            situation: Some("일상".into()),
        }
    }

    fn glam_look(outer_role: Role) -> OutfitContext {
        ctx(vec![
            slot(SlotKind::Top, "에메랄드 새틴 블라우스", Role::Accent, 3),
            slot(SlotKind::Bottom, "블랙 슬랙스", Role::Base, 4),
            slot(SlotKind::Outer, "블랙 자켓", outer_role, 4),
            slot(SlotKind::Shoes, "블랙 펌프스", Role::Base, 4),
            slot(SlotKind::Bag, "블랙 클러치", Role::Connector, 3),
        ])
    }

    /// 조용한 아우터 안의 포인트 상의는 이너·슬롯 감점을 되돌린다. 아우터도 포인트면
    /// 경쟁이 실제로 있으므로 그대로 둔다.
    #[test]
    fn a_strong_top_is_only_penalised_when_the_outer_is_strong_too() {
        let quiet = glam_look(Role::Structural);
        let e = style_engine::evaluate(&quiet, None);
        assert_eq!(waiver(&quiet, &e, None).inner, 18);

        let loud = glam_look(Role::Accent);
        let e = style_engine::evaluate(&loud, None);
        assert_eq!(waiver(&loud, &e, None).inner, 0);
    }

    /// 되돌린 점수는 운영 상한을 넘지 않는다.
    #[test]
    fn waived_score_never_exceeds_the_ceiling() {
        for role in [Role::Structural, Role::Connector, Role::Accent] {
            let look = glam_look(role);
            let e = style_engine::evaluate(&look, None);
            let w = waiver(&look, &e, Some(StyleGenre::Glam));
            assert!(e.score + w.total() <= style_engine::SCORE_CEILING);
        }
    }
}
