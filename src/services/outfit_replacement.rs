//! 코디에서 한 벌만 바꾸기: 요청 해석과 후보 판정.
//!
//! 고른 자리 말고는 아무것도 바꾸지 않는다. 후보는 내 옷장의 같은 카테고리 옷뿐이고, 모델을 부르지
//! 않으며, 저장·착용 기록도 하지 않는다 — 대안마다 새 참조만 돌려준다.
//!
//! 조건은 저장된 속성으로 판단할 수 있는 것만 받는다([`Preference`]). 판단할 수 없는 조건(방수,
//! 미끄럼 방지, 세탁 상태 등)이나 알아듣지 못한 말은 버리지 않고 [`Intent::Clarify`] 로 되묻는다.
//! '비 오는 날 신발' 을 조건 없이 아무 신발로 답하면 사용자는 방수를 확인한 줄 안다.
//!
//! 한계: '편한 신발' 은 착화감 데이터가 없어 세부 카테고리(스니커즈·로퍼·플랫 등)로만 판단한다.
//! 두께·밝기·격식은 저장된 값 그대로이며 값이 없는 옷은 그 조건의 후보에서 뺀다.

use serde::Serialize;

use crate::models::clothing::Clothing;

/// 저장된 속성으로 판단할 수 있는 조건.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Preference {
    Comfortable,
    Warmer,
    Lighter,
    Darker,
    Brighter,
    MoreFormal,
    MoreCasual,
}

impl Preference {
    pub const ALL: [Preference; 7] = [
        Preference::Comfortable,
        Preference::Warmer,
        Preference::Lighter,
        Preference::Darker,
        Preference::Brighter,
        Preference::MoreFormal,
        Preference::MoreCasual,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Preference::Comfortable => "comfortable",
            Preference::Warmer => "warmer",
            Preference::Lighter => "lighter",
            Preference::Darker => "darker",
            Preference::Brighter => "brighter",
            Preference::MoreFormal => "more_formal",
            Preference::MoreCasual => "more_casual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == s)
    }

    fn label(self) -> &'static str {
        match self {
            Preference::Comfortable => "편한 신발",
            Preference::Warmer => "더 따뜻한 옷",
            Preference::Lighter => "더 얇은 옷",
            Preference::Darker => "더 어두운 색",
            Preference::Brighter => "더 밝은 색",
            Preference::MoreFormal => "더 격식 있는 옷",
            Preference::MoreCasual => "더 캐주얼한 옷",
        }
    }

    /// 이 조건을 판단할 수 있는 카테고리인가.
    pub fn supports(self, category: &str) -> bool {
        match self {
            Preference::Comfortable => category == "신발",
            Preference::Warmer | Preference::Lighter => {
                matches!(category, "상의" | "하의" | "아우터")
            }
            _ => true,
        }
    }

    /// 후보가 조건을 만족하면 근거 문장을 돌려준다. 값을 모르면 만족하지 않는다.
    pub fn check(self, candidate: &Clothing, current: &Clothing) -> Option<String> {
        match self {
            Preference::Comfortable => comfortable_kind(candidate)
                .map(|k| format!("{k} 유형이라 편한 쪽으로 골랐어요(착화감을 잰 값은 아니에요).")),
            Preference::Warmer => (candidate.thickness > current.thickness).then(|| {
                format!(
                    "두께가 {} → {} 로 더 두꺼워요.",
                    thickness_label(current),
                    thickness_label(candidate)
                )
            }),
            Preference::Lighter => (candidate.thickness < current.thickness).then(|| {
                format!(
                    "두께가 {} → {} 로 더 얇아요.",
                    thickness_label(current),
                    thickness_label(candidate)
                )
            }),
            Preference::Darker => tone_gap(candidate, current)
                .filter(|(c, o)| c > o)
                .map(|(c, o)| format!("밝기가 {} → {} 로 더 어두워요.", o.as_str(), c.as_str())),
            Preference::Brighter => tone_gap(candidate, current)
                .filter(|(c, o)| c < o)
                .map(|(c, o)| format!("밝기가 {} → {} 로 더 밝아요.", o.as_str(), c.as_str())),
            Preference::MoreFormal => formality_gap(candidate, current)
                .filter(|(c, o)| c > o)
                .map(|(c, o)| format!("격식 값이 {o} → {c} 로 올라가요.")),
            Preference::MoreCasual => formality_gap(candidate, current)
                .filter(|(c, o)| c < o)
                .map(|(c, o)| format!("격식 값이 {o} → {c} 로 내려가요.")),
        }
    }
}

fn thickness_label(c: &Clothing) -> &'static str {
    use crate::models::style_vocab::Thickness as T;
    match c.thickness {
        T::Thin => "얇음",
        T::Medium => "보통",
        T::Thick => "두꺼움",
    }
}

fn tone_gap(
    a: &Clothing,
    b: &Clothing,
) -> Option<(
    crate::models::style_vocab::Tone,
    crate::models::style_vocab::Tone,
)> {
    Some((a.tone?, b.tone?))
}

fn formality_gap(a: &Clothing, b: &Clothing) -> Option<(i8, i8)> {
    Some((a.formality_level?, b.formality_level?))
}

/// 굽이 낮고 발을 덜 조이는 신발 유형. 세부 카테고리 값(실제 옷장의 한글·영문 값)으로 본다.
fn comfortable_kind(c: &Clothing) -> Option<&'static str> {
    if c.category != "신발" {
        return None;
    }
    let sub = c.sub_category.as_deref().unwrap_or("").to_lowercase();
    const KINDS: [(&str, &str); 12] = [
        ("스니커즈", "스니커즈"),
        ("sneaker", "스니커즈"),
        ("runner", "러닝화"),
        ("trainer", "트레이너"),
        ("slip_on", "슬립온"),
        ("슬립온", "슬립온"),
        ("로퍼", "로퍼"),
        ("loafer", "로퍼"),
        ("플랫", "플랫슈즈"),
        ("발레리나", "발레리나 플랫"),
        ("에스파드리유", "에스파드리유"),
        ("trail", "트레일화"),
    ];
    KINDS
        .iter()
        .find(|(needle, _)| sub.contains(needle))
        .map(|(_, label)| *label)
}

/// 요청을 해석한 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum Intent {
    /// 사용자가 고른 옷으로 바꾼다.
    Explicit(String),
    /// 조건에 맞는 옷을 찾는다. `None` 이면 조건 없이 같은 카테고리의 다른 옷이다.
    Find(Option<Preference>),
    /// 되물어야 한다. 조건을 버리고 진행하지 않는다.
    Clarify {
        reason: &'static str,
        message: String,
    },
}

/// 지원 조건 목록을 사람 말로.
pub fn supported_list() -> String {
    Preference::ALL
        .iter()
        .map(|p| format!("{}({})", p.label(), p.key()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 저장된 속성으로는 확인할 수 없는 조건을 가리키는 말.
const UNSUPPORTED_WORDS: [&str; 22] = [
    "방수",
    "비오",
    "비올",
    "비에",
    "우천",
    "장마",
    "rain",
    "waterproof",
    "방풍",
    "눈길",
    "눈오",
    "snow",
    "미끄",
    "논슬립",
    "세탁",
    "빨래",
    "구매",
    "쇼핑",
    "사고",
    "가격",
    "새로산",
    "새옷",
];

/// 사용자 말을 조건으로 옮긴다. 공백을 뗀 전체 문장이 표에 있을 때만 받는다 — 일부만 맞는 말을
/// 조건으로 삼으면 나머지 말이 조용히 버려진다.
fn phrase_preference(text: &str) -> Option<Option<Preference>> {
    use Preference::*;
    const PHRASES: [(&str, Option<Preference>); 30] = [
        ("편한", Some(Comfortable)),
        ("편한거", Some(Comfortable)),
        ("편한걸로", Some(Comfortable)),
        ("편하게", Some(Comfortable)),
        ("편한신발", Some(Comfortable)),
        ("편한신발로", Some(Comfortable)),
        ("comfortable", Some(Comfortable)),
        ("따뜻한", Some(Warmer)),
        ("더따뜻한", Some(Warmer)),
        ("따뜻하게", Some(Warmer)),
        ("warmer", Some(Warmer)),
        ("얇은", Some(Lighter)),
        ("더얇은", Some(Lighter)),
        ("시원한", Some(Lighter)),
        ("시원하게", Some(Lighter)),
        ("lighter", Some(Lighter)),
        ("어두운", Some(Darker)),
        ("더어두운", Some(Darker)),
        ("darker", Some(Darker)),
        ("밝은", Some(Brighter)),
        ("더밝은", Some(Brighter)),
        ("brighter", Some(Brighter)),
        ("더단정한", Some(MoreFormal)),
        ("격식있는", Some(MoreFormal)),
        ("moreformal", Some(MoreFormal)),
        ("더캐주얼한", Some(MoreCasual)),
        ("캐주얼한", Some(MoreCasual)),
        ("다른거", None),
        ("다른걸로", None),
        ("아무거나", None),
    ];
    PHRASES.iter().find(|(p, _)| *p == text).map(|(_, v)| *v)
}

fn clarify(reason: &'static str, message: String) -> Intent {
    Intent::Clarify { reason, message }
}

/// 요청을 해석한다.
pub fn parse_intent(
    category: &str,
    replacement_item_id: Option<&str>,
    preference: Option<&str>,
    request_text: Option<&str>,
) -> Intent {
    let text = request_text
        .map(|t| {
            t.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|t| !t.is_empty());
    // 판단할 수 없는 조건이 하나라도 있으면 다른 값이 있어도 먼저 되묻는다.
    if let Some(t) = &text
        && UNSUPPORTED_WORDS.iter().any(|w| t.contains(w))
    {
        return clarify(
            "unsupported_constraint",
            format!(
                "방수·미끄럼·세탁 상태·구매 같은 조건은 옷장 정보로 확인할 수 없어 적용하지 못해요. \
                 조건을 빼고 고를지, 쓸 수 있는 조건으로 바꿀지 알려 주세요. 쓸 수 있는 조건: {}",
                supported_list()
            ),
        );
    }
    let from_text = match &text {
        Some(t) => match phrase_preference(t) {
            Some(p) => Some(p),
            None => {
                return clarify(
                    "ambiguous_request",
                    format!(
                        "요청을 조건으로 옮기지 못했어요. 옷 id 를 고르거나 쓸 수 있는 조건으로 말해 주세요: {}",
                        supported_list()
                    ),
                );
            }
        },
        None => None,
    };
    let from_key = match preference {
        Some(raw) => match Preference::parse(raw) {
            Some(p) => Some(p),
            None => {
                return clarify(
                    "unsupported_preference",
                    format!(
                        "지원하지 않는 조건이에요. 쓸 수 있는 조건: {}",
                        supported_list()
                    ),
                );
            }
        },
        None => None,
    };
    let pref = match (from_key, from_text) {
        (Some(a), Some(Some(b))) if a != b => {
            return clarify(
                "ambiguous_request",
                "조건과 요청 문장이 서로 달라요. 어느 쪽인지 알려 주세요.".to_string(),
            );
        }
        (Some(a), _) => Some(a),
        (None, Some(b)) => b,
        (None, None) => None,
    };
    if let Some(id) = replacement_item_id {
        if pref.is_some() {
            return clarify(
                "ambiguous_request",
                "옷을 직접 고르거나 조건으로 찾거나 둘 중 하나만 정해 주세요.".to_string(),
            );
        }
        return Intent::Explicit(id.to_string());
    }
    if let Some(p) = pref
        && !p.supports(category)
    {
        return clarify(
            "unsupported_for_category",
            format!("'{}' 조건은 {category}에는 판단할 수 없어요.", p.label()),
        );
    }
    Intent::Find(pref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::style_vocab::{Thickness, Tone};

    fn item(category: &str, sub: Option<&str>) -> Clothing {
        let t = chrono::NaiveDateTime::default();
        Clothing {
            id: "x".into(),
            name: "x".into(),
            category: category.into(),
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
            silhouette_volume: None,
            material_primary: None,
            sub_category: sub.map(str::to_string),
            floating_score: None,
            strong_style_score: None,
            texture_keywords: None,
            created_at: t,
            updated_at: t,
        }
    }

    #[test]
    fn unsupported_constraints_are_never_dropped() {
        for text in [
            "방수 되는 걸로",
            "비 오는 날 신발",
            "rainproof",
            "편한 방수 신발",
        ] {
            let got = parse_intent("신발", None, Some("comfortable"), Some(text));
            assert!(
                matches!(
                    got,
                    Intent::Clarify {
                        reason: "unsupported_constraint",
                        ..
                    }
                ),
                "{text}: {got:?}"
            );
            // 옷을 직접 골라도 같은 요청의 조건을 버리지 않는다.
            let got = parse_intent("신발", Some("s2"), None, Some(text));
            assert!(matches!(got, Intent::Clarify { .. }), "{text}: {got:?}");
        }
    }

    #[test]
    fn unknown_words_ask_back_instead_of_guessing() {
        let got = parse_intent("상의", None, None, Some("좀 힙한 느낌으로"));
        assert!(matches!(
            got,
            Intent::Clarify {
                reason: "ambiguous_request",
                ..
            }
        ));
        let got = parse_intent("상의", None, Some("fancy"), None);
        assert!(matches!(
            got,
            Intent::Clarify {
                reason: "unsupported_preference",
                ..
            }
        ));
    }

    #[test]
    fn supported_words_and_keys_resolve() {
        assert_eq!(
            parse_intent("신발", None, None, Some("편한 신발로")),
            Intent::Find(Some(Preference::Comfortable))
        );
        assert_eq!(
            parse_intent("신발", None, Some("comfortable"), None),
            Intent::Find(Some(Preference::Comfortable))
        );
        assert_eq!(parse_intent("하의", None, None, None), Intent::Find(None));
        assert_eq!(
            parse_intent("하의", None, None, Some("다른 거")),
            Intent::Find(None)
        );
        assert_eq!(
            parse_intent("신발", Some("s9"), None, None),
            Intent::Explicit("s9".into())
        );
    }

    #[test]
    fn preference_must_fit_the_category_and_not_contradict() {
        assert!(matches!(
            parse_intent("상의", None, Some("comfortable"), None),
            Intent::Clarify {
                reason: "unsupported_for_category",
                ..
            }
        ));
        assert!(matches!(
            parse_intent("신발", None, Some("warmer"), None),
            Intent::Clarify {
                reason: "unsupported_for_category",
                ..
            }
        ));
        assert!(matches!(
            parse_intent("상의", None, Some("darker"), Some("밝은")),
            Intent::Clarify {
                reason: "ambiguous_request",
                ..
            }
        ));
        assert!(matches!(
            parse_intent("상의", Some("t1"), Some("darker"), None),
            Intent::Clarify {
                reason: "ambiguous_request",
                ..
            }
        ));
    }

    #[test]
    fn comfortable_uses_shoe_kind_only() {
        let cur = item("신발", Some("힐"));
        assert!(
            Preference::Comfortable
                .check(&item("신발", Some("canvas_sneaker")), &cur)
                .is_some()
        );
        assert!(
            Preference::Comfortable
                .check(&item("신발", Some("로퍼")), &cur)
                .is_some()
        );
        for sub in [Some("힐"), Some("앵클부츠"), Some("샌들"), None] {
            assert!(
                Preference::Comfortable
                    .check(&item("신발", sub), &cur)
                    .is_none(),
                "{sub:?}"
            );
        }
    }

    #[test]
    fn attribute_preferences_compare_with_the_current_item() {
        let mut cur = item("상의", None);
        let mut thick = item("상의", None);
        thick.thickness = Thickness::Thick;
        assert!(Preference::Warmer.check(&thick, &cur).is_some());
        assert!(Preference::Lighter.check(&thick, &cur).is_none());

        // 밝기를 모르면 맞다고 하지 않는다.
        let mut dark = item("상의", None);
        dark.tone = Some(Tone::Dark);
        assert!(Preference::Darker.check(&dark, &cur).is_none());
        cur.tone = Some(Tone::Mid);
        assert!(Preference::Darker.check(&dark, &cur).is_some());
        assert!(Preference::Brighter.check(&dark, &cur).is_none());
    }
}
