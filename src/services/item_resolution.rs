//! LLM·클라이언트가 돌려준 옷을 옷장 아이템에 연결하는 규칙(2026-10-18).
//!
//! 예전에는 이름으로 연결했다 — 정확히 같은 이름, 없으면 부분 일치의 첫 아이템
//! (`recommendation::find_matching_clothing`). 모델이 "울혼방" 을 "울혼합" 으로 적으면 후보가
//! 통째로 빠졌고(2026-10-18 실측 4개), 이름이 비슷한 다른 옷이 조용히 붙을 수도 있었다. 같은
//! 이름의 옷이 둘이면 구분하지 못했다.
//!
//! 이제 서버가 후보 목록에 실제 의류 id 를 넣어 보여 주고, 응답의 `id` 를 권위 있는 선택값으로
//! 쓴다. 이름은 표시용으로 옷장에서 다시 가져온다.
//!
//! - id 는 **이 요청에서 그 슬롯에 제공한 목록 안에서만** 찾는다. 전역 DB 로 찾지 않는다 —
//!   다른 옷장·무드·슬롯 제한을 우회하지 않는다.
//! - id 가 틀리면(목록에 없음·다른 슬롯) 이름이 맞아도 이름으로 조용히 복구하지 않고 거절한다.
//! - 신규 응답에서 id 가 빠지면 거절한다. id 도 이름도 비어 있으면 그 슬롯을 비운 정상 생략이다
//!   (아우터가 필요 없는 날 등).
//! - 이름만 있는 예전 기록(구버전 클라이언트가 돌려보낸 상담 코디 등)은 `resolve_legacy_name` 으로만
//!   받는다: 정확히 같은 이름이 하나일 때만 연결하고, 둘 이상이면 거절한다. 부분 일치는 쓰지 않는다.
//!
//! 사용자의 자유 텍스트 검색("이 니트 바꿔줘")은 여기서 다루지 않는다 — 별도 검색·확정 단계다.

use crate::models::clothing::Clothing;
use crate::models::outfit::SlotKind;

/// 옷장 카테고리 → 슬롯.
pub fn slot_of_category(category: &str) -> Option<SlotKind> {
    match category.trim() {
        "상의" => Some(SlotKind::Top),
        "하의" => Some(SlotKind::Bottom),
        "아우터" => Some(SlotKind::Outer),
        "신발" => Some(SlotKind::Shoes),
        "가방" => Some(SlotKind::Bag),
        _ => None,
    }
}

/// 이 요청에서 슬롯마다 제공한 옷. id 는 여기 안에서만 찾는다.
#[derive(Debug, Default)]
pub struct OfferedItems<'a> {
    items: Vec<(SlotKind, &'a Clothing)>,
}

impl<'a> OfferedItems<'a> {
    /// 옷마다 자기 카테고리의 슬롯에 둔다. 슬롯이 없는 카테고리는 넣지 않는다.
    pub fn from_clothes<I: IntoIterator<Item = &'a Clothing>>(clothes: I) -> Self {
        Self {
            items: clothes
                .into_iter()
                .filter_map(|c| slot_of_category(&c.category).map(|s| (s, c)))
                .collect(),
        }
    }

    /// id 의 아이템과 그 슬롯.
    fn find(&self, id: &str) -> Option<(SlotKind, &'a Clothing)> {
        self.items.iter().find(|(_, c)| c.id == id).copied()
    }
}

/// 연결하지 못한 이유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// 카테고리를 슬롯으로 읽을 수 없다.
    UnknownCategory(String),
    /// 신규 응답인데 id 가 없다(이름만 있다).
    MissingId,
    /// 이 요청에서 제공하지 않은 id 다(다른 옷장·무드·목록 밖, 또는 지어낸 값).
    NotOffered { id: String },
    /// 제공한 id 지만 다른 슬롯의 목록에 있다.
    SlotMismatch {
        id: String,
        offered_in: SlotKind,
        claimed: SlotKind,
    },
}

impl Rejection {
    /// 로그용 짧은 이름.
    pub fn code(&self) -> &'static str {
        match self {
            Rejection::UnknownCategory(_) => "unknown_category",
            Rejection::MissingId => "missing_id",
            Rejection::NotOffered { .. } => "id_not_offered",
            Rejection::SlotMismatch { .. } => "slot_mismatch",
        }
    }
}

/// 한 항목의 연결 결과.
#[derive(Debug)]
pub enum Resolution<'a> {
    /// 연결했다. `name_differs` 는 모델이 적은 이름이 옷장 이름과 다르다는 관측이다(선택에는
    /// 영향이 없다 — 표시 이름은 옷장 것을 쓴다).
    Resolved {
        clothing: &'a Clothing,
        slot: SlotKind,
        name_differs: bool,
    },
    /// id 도 이름도 비어 있다 — 그 슬롯을 비운 정상 생략.
    Omitted,
    Rejected(Rejection),
}

/// 연결 결과는 옷 id 로 비교한다(`Clothing` 은 비교 연산이 없다).
impl PartialEq for Resolution<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Resolution::Resolved {
                    clothing: a,
                    slot: sa,
                    name_differs: na,
                },
                Resolution::Resolved {
                    clothing: b,
                    slot: sb,
                    name_differs: nb,
                },
            ) => a.id == b.id && sa == sb && na == nb,
            (Resolution::Omitted, Resolution::Omitted) => true,
            (Resolution::Rejected(a), Resolution::Rejected(b)) => a == b,
            _ => false,
        }
    }
}

/// 신규 응답(LLM 후보·서버가 만든 후보)의 한 항목을 연결한다.
pub fn resolve_item<'a>(
    category: &str,
    id: Option<&str>,
    name: &str,
    offered: &OfferedItems<'a>,
) -> Resolution<'a> {
    let Some(claimed) = slot_of_category(category) else {
        return Resolution::Rejected(Rejection::UnknownCategory(category.to_string()));
    };
    let id = id.map(str::trim).filter(|s| !s.is_empty());
    let name = name.trim();
    let Some(id) = id else {
        return if name.is_empty() {
            Resolution::Omitted
        } else {
            Resolution::Rejected(Rejection::MissingId)
        };
    };
    match offered.find(id) {
        None => Resolution::Rejected(Rejection::NotOffered { id: id.to_string() }),
        Some((slot, _)) if slot != claimed => Resolution::Rejected(Rejection::SlotMismatch {
            id: id.to_string(),
            offered_in: slot,
            claimed,
        }),
        Some((slot, clothing)) => Resolution::Resolved {
            clothing,
            slot,
            name_differs: !name.is_empty() && name != clothing.name,
        },
    }
}

/// 이름만 있는 예전 기록을 연결하지 못한 이유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyRejection {
    NotFound,
    /// 같은 이름의 옷이 여럿이다 — 어느 것인지 모른다.
    Ambiguous(usize),
}

/// id 가 없는 예전 기록 전용: 정확히 같은 이름이 하나일 때만 연결한다. 부분 일치는 쓰지 않는다.
pub fn resolve_legacy_name<'a, I>(name: &str, pool: I) -> Result<&'a Clothing, LegacyRejection>
where
    I: IntoIterator<Item = &'a Clothing>,
{
    let name = name.trim();
    if name.is_empty() {
        return Err(LegacyRejection::NotFound);
    }
    let mut found = pool.into_iter().filter(|c| c.name == name);
    match (found.next(), found.count()) {
        (None, _) => Err(LegacyRejection::NotFound),
        (Some(c), 0) => Ok(c),
        (Some(_), more) => Err(LegacyRejection::Ambiguous(more + 1)),
    }
}

/// id 가 있으면 id 로(제공 목록 안에서만), 없을 때만 예전 이름 규칙으로 연결한다. id 가 있는데
/// 목록에 없으면 이름으로 복구하지 않는다.
pub fn resolve_id_or_legacy<'a>(
    id: Option<&str>,
    name: &str,
    pool: &'a [Clothing],
) -> Option<&'a Clothing> {
    match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => pool.iter().find(|c| c.id == id),
        None => resolve_legacy_name(name, pool.iter()).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, name: &str, category: &str) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            color: None,
            thickness: crate::models::style_vocab::Thickness::Medium,
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
        }
    }

    fn wardrobe() -> Vec<Clothing> {
        vec![
            item("top-a", "블랙 리브드 슬림 니트", "상의"),
            item("top-b", "블랙 리브드 슬림 니트", "상의"), // 같은 이름, 다른 옷
            item("bot-a", "블랙 스트레이트 울혼방 슬랙스", "하의"),
            item("out-a", "블랙 레더 라이더 자켓", "아우터"),
            item("not-offered", "그레이 니트 터틀넥 탑", "상의"),
        ]
    }

    /// 같은 이름의 두 옷은 id 로 구분된다.
    #[test]
    fn same_name_different_ids_stay_distinct() {
        let w = wardrobe();
        let offered = OfferedItems::from_clothes(&w[..4]);
        for id in ["top-a", "top-b"] {
            match resolve_item("상의", Some(id), "블랙 리브드 슬림 니트", &offered) {
                Resolution::Resolved { clothing, .. } => assert_eq!(clothing.id, id),
                other => panic!("{other:?}"),
            }
        }
    }

    /// 모델이 이름을 잘못 적어도(울혼방→울혼합) id 가 맞으면 같은 옷이다. 바뀐 이름도 같다.
    #[test]
    fn a_typo_or_renamed_item_resolves_by_id() {
        let w = wardrobe();
        let offered = OfferedItems::from_clothes(&w[..4]);
        for name in [
            "블랙 스트레이트 울혼합 슬랙스",
            "블랙 와이드 슬랙스(예전 이름)",
            "",
        ] {
            match resolve_item("하의", Some("bot-a"), name, &offered) {
                Resolution::Resolved {
                    clothing,
                    name_differs,
                    ..
                } => {
                    assert_eq!(clothing.id, "bot-a");
                    assert_eq!(clothing.name, "블랙 스트레이트 울혼방 슬랙스");
                    assert_eq!(name_differs, !name.is_empty(), "{name}");
                }
                other => panic!("{name}: {other:?}"),
            }
        }
    }

    /// 틀린 id + 맞는 이름은 이름으로 복구하지 않는다. 제공하지 않은 옷(전역 DB 에 있어도),
    /// 다른 슬롯의 id 도 거절한다.
    #[test]
    fn wrong_or_unoffered_ids_are_rejected_even_with_a_right_name() {
        let w = wardrobe();
        let offered = OfferedItems::from_clothes(&w[..4]);
        assert_eq!(
            resolve_item(
                "하의",
                Some("bot-zzz"),
                "블랙 스트레이트 울혼방 슬랙스",
                &offered
            ),
            Resolution::Rejected(Rejection::NotOffered {
                id: "bot-zzz".into()
            })
        );
        assert_eq!(
            resolve_item(
                "상의",
                Some("not-offered"),
                "그레이 니트 터틀넥 탑",
                &offered
            ),
            Resolution::Rejected(Rejection::NotOffered {
                id: "not-offered".into()
            })
        );
        assert_eq!(
            resolve_item("상의", Some("out-a"), "블랙 레더 라이더 자켓", &offered),
            Resolution::Rejected(Rejection::SlotMismatch {
                id: "out-a".into(),
                offered_in: SlotKind::Outer,
                claimed: SlotKind::Top,
            })
        );
        assert_eq!(
            resolve_item("모자", Some("top-a"), "x", &offered),
            Resolution::Rejected(Rejection::UnknownCategory("모자".into()))
        );
    }

    /// 신규 응답에서 id 가 빠지면 이름이 맞아도 거절한다. id·이름이 모두 비면 정상 생략이다.
    #[test]
    fn missing_ids_are_rejected_but_empty_slots_are_omitted() {
        let w = wardrobe();
        let offered = OfferedItems::from_clothes(&w[..4]);
        assert_eq!(
            resolve_item("아우터", None, "블랙 레더 라이더 자켓", &offered),
            Resolution::Rejected(Rejection::MissingId)
        );
        assert_eq!(
            resolve_item("아우터", Some(""), "", &offered),
            Resolution::Omitted
        );
        assert_eq!(
            resolve_item("아우터", None, " ", &offered),
            Resolution::Omitted
        );
    }

    /// 예전 기록(이름만): 정확히 같은 이름이 하나일 때만. 같은 이름이 둘이면 거절, 부분 일치는
    /// 쓰지 않는다. id 가 있으면 id 로만 찾고 이름으로 복구하지 않는다.
    #[test]
    fn legacy_names_need_a_unique_exact_match() {
        let w = wardrobe();
        assert_eq!(
            resolve_legacy_name("블랙 레더 라이더 자켓", &w).map(|c| c.id.as_str()),
            Ok("out-a")
        );
        assert_eq!(
            resolve_legacy_name("블랙 리브드 슬림 니트", &w).map(|c| c.id.as_str()),
            Err(LegacyRejection::Ambiguous(2))
        );
        assert_eq!(
            resolve_legacy_name("라이더 자켓", &w).map(|c| c.id.as_str()),
            Err(LegacyRejection::NotFound)
        );
        assert_eq!(
            resolve_id_or_legacy(Some("top-b"), "아무 이름", &w).map(|c| c.id.as_str()),
            Some("top-b")
        );
        assert!(resolve_id_or_legacy(Some("gone"), "블랙 레더 라이더 자켓", &w).is_none());
        assert_eq!(
            resolve_id_or_legacy(None, "블랙 레더 라이더 자켓", &w).map(|c| c.id.as_str()),
            Some("out-a")
        );
        assert!(resolve_id_or_legacy(None, "블랙 리브드 슬림 니트", &w).is_none());
    }
}
