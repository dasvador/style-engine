//! 일부 성별·무드의 색 다양성(2026-10-17 사용자 요청 — 여성 스트리트·스포티 캐주얼, 남성
//! 아메카지·프레피).
//!
//! 실제 홈 추천 51회 점검(`mood-color-audit-20261017`)에서 이 네 조합의 최종 카드는 상의·
//! 아우터가 전부 기본색이었다. 관찰한 탈락 지점은 조합마다 다르다 — 여성 두 무드는 옷장에 컬러
//! 옷이 없었고(시드로 보강), 남성 아메카지는 컬러 셔츠가 후보 목록에서 먼저 잘렸고(`shortlist`),
//! 프레피는 후보 목록에 하늘색 셔츠가 있어도 초안이 기본색만 골랐다(`prompts` — 원인은 추정). 로맨틱·보헤미안처럼 이미
//! 색이 나오는 무드와 다른 성별의 같은 이름 무드는 건드리지 않는다.
//!
//! 색은 저장된 색 이름으로만 본다. 엣지의 `recommendation_service::is_chromatic` 은 채도
//! "낮음" 을 무채색으로 치지만, 이 무드들의 컬러는 하늘색·라벤더·세이지처럼 파스텔이 많아
//! 채도로 거르지 않는다. 대신 기본색(검정·흰색·회색·베이지·브라운·네이비·데님·올리브·카키 등)과
//! 이름을 모르는 색은 색 계열이 없다(`None`). 엣지 판정은 바꾸지 않는다.

use crate::models::clothing::Clothing;
use crate::models::style_vocab::StyleGenre;

/// 이 요청에서 색 다양성을 켜는가. 대상 성별·무드이고, 사용자가 밝은색을 싫어한다고 적지
/// 않았을 때만이다(`user_style_profile.dislikes_bright_colors`).
pub fn enabled(gender: Option<&str>, mood: Option<StyleGenre>, dislikes_bright: bool) -> bool {
    targets(gender, mood) && !dislikes_bright
}

/// 색 다양성을 다루는 성별·무드인가. 같은 무드 이름이라도 다른 성별이면 아니다.
pub fn targets(gender: Option<&str>, mood: Option<StyleGenre>) -> bool {
    matches!(
        (gender, mood),
        (
            Some("female"),
            Some(StyleGenre::Street | StyleGenre::SportyCasual)
        ) | (Some("male"), Some(StyleGenre::Amekaji | StyleGenre::Preppy))
    )
}

/// 기본색. 이름에 이 말이 있으면 색 계열이 없다. 색 계열 이름보다 먼저 본다 — "네이비블루",
/// "다크블루", "잉크블루" 는 네이비이고 "인디고" 는 데님이다.
const BASIC_WORDS: [&str; 30] = [
    "블랙",
    "차콜",
    "그레이",
    "그레이지",
    "화이트",
    "실버",
    "아이보리",
    "크림",
    "오트밀",
    "에크루",
    "베이지",
    "샌드",
    "스톤",
    "브라운",
    "카멜",
    "탄",
    "모카",
    "에스프레소",
    "네이비",
    "다크블루",
    "잉크",
    "인디고",
    "데님",
    "올리브",
    "카키",
    "black",
    "white",
    "gray",
    "grey",
    "navy",
];

/// 색 계열과 그 계열을 가리키는 이름. 앞에서부터 본다 — "버건디" 는 레드, "라벤더" 는 퍼플.
const FAMILIES: [(&str, &[&str]); 8] = [
    (
        "레드",
        &[
            "레드",
            "체리",
            "버건디",
            "와인",
            "브릭",
            "라즈베리",
            "마룬",
            "red",
        ],
    ),
    ("핑크", &["핑크", "코럴", "pink"]),
    (
        "오렌지",
        &["오렌지", "러스트", "테라코타", "주황", "orange", "rust"],
    ),
    ("옐로", &["옐로", "머스타드", "노랑", "라임", "yellow"]),
    (
        "그린",
        &[
            "그린",
            "포레스트",
            "세이지",
            "민트",
            "에메랄드",
            "초록",
            "green",
        ],
    ),
    ("청록", &["틸", "청록", "아쿠아", "teal"]),
    (
        "블루",
        &[
            "블루",
            "하늘",
            "스카이",
            "코발트",
            "샴브레이",
            "페트롤",
            "파랑",
            "blue",
        ],
    ),
    (
        "퍼플",
        &[
            "퍼플",
            "바이올렛",
            "라벤더",
            "라일락",
            "플럼",
            "보라",
            "purple",
        ],
    ),
];

/// 배색 색 이름을 나누는 구분자. 업로드 분석은 "코발트/오프화이트", "red/black" 처럼 적는다.
const COLOUR_SEPARATORS: [char; 6] = ['/', ',', '&', '+', '·', '|'];

/// 옷의 색 계열. 기본색이거나 색 이름이 없거나 모르는 이름이면 `None` 이다 — 모르는 색을
/// 컬러로 치지 않는다. 채도는 보지 않는다(파스텔도 색이다).
///
/// 배색("코발트/오프화이트")은 구분자로 나뉜 성분을 하나씩 보고, 앞에서부터 처음 색 계열이 있는
/// 성분을 쓴다. 예전에는 문자열 전체에서 기본색 단어를 먼저 찾아, 화이트가 섞인 배색이 전부
/// `None` 이 됐다. 성분 하나는 예전 규칙 그대로다 — "네이비블루" 는 여전히 네이비다. 성분이 모두
/// 기본색이거나 모르는 이름이면 `None` 이다.
pub fn colour_family(c: &Clothing) -> Option<&'static str> {
    let color = c.color.as_deref()?.trim().to_lowercase();
    color.split(COLOUR_SEPARATORS).find_map(component_family)
}

fn component_family(part: &str) -> Option<&'static str> {
    let part = part.trim();
    if part.is_empty() || BASIC_WORDS.iter().any(|w| part.contains(w)) {
        return None;
    }
    FAMILIES
        .iter()
        .find(|(_, words)| words.iter().any(|w| part.contains(w)))
        .map(|(family, _)| *family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(color: Option<&str>) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: "x".into(),
            name: "x".into(),
            category: "상의".into(),
            color: color.map(Into::into),
            thickness: crate::models::style_vocab::Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: Some(crate::models::style_vocab::Saturation::Low),
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

    /// 파스텔(채도 낮음)도 색이다. 기본색·모르는 이름·빈 값은 색 계열이 없다.
    #[test]
    fn families_come_from_the_colour_name_not_saturation() {
        for (color, family) in [
            ("라이트블루", "블루"),
            ("하늘색", "블루"),
            ("슬레이트블루", "블루"),
            ("라벤더", "퍼플"),
            ("세이지", "그린"),
            ("브릭레드", "레드"),
            ("러스트", "오렌지"),
            ("소프트핑크", "핑크"),
            ("틸", "청록"),
            ("머스타드", "옐로"),
        ] {
            assert_eq!(colour_family(&item(Some(color))), Some(family), "{color}");
        }
        for color in [
            "블랙",
            "워시드블랙",
            "헤더그레이",
            "네이비",
            "다크블루",
            "잉크블루",
            "인디고",
            "카키",
            "올리브",
            "베이지",
            "브라운",
            "오트밀",
            "미상",
            "unknown",
            "",
            "로즈골드",
            "샴페인",
        ] {
            assert_eq!(colour_family(&item(Some(color))), None, "{color}");
        }
        assert_eq!(colour_family(&item(None)), None);
    }

    /// 배색은 구분자로 나뉜 성분을 하나씩 본다. 기본색이 섞여도 색 성분이 있으면 그 계열이고,
    /// 모든 성분이 기본색·미상이면 없다. 구분자 없이 붙은 "네이비블루" 는 여전히 네이비다.
    #[test]
    fn two_tone_colours_are_judged_part_by_part() {
        for (color, family) in [
            ("코발트/오프화이트", Some("블루")),
            ("블루/화이트", Some("블루")),
            ("red/black", Some("레드")),
            ("블랙 / 라벤더", Some("퍼플")),
            ("화이트·포레스트그린", Some("그린")),
            ("네이비블루", None),
            ("네이비/화이트", None),
            ("인디고/미상", None),
            ("미상/unknown", None),
            ("블랙/그레이", None),
        ] {
            assert_eq!(colour_family(&item(Some(color))), family, "{color}");
        }
    }

    /// 대상은 네 성별·무드뿐이다. 같은 이름의 무드라도 다른 성별이면 아니다.
    #[test]
    fn only_the_four_gender_mood_pairs_are_targeted() {
        use StyleGenre::*;
        assert!(targets(Some("female"), Some(Street)));
        assert!(targets(Some("female"), Some(SportyCasual)));
        assert!(targets(Some("male"), Some(Amekaji)));
        assert!(targets(Some("male"), Some(Preppy)));
        assert!(!targets(Some("male"), Some(Street)));
        assert!(!targets(Some("male"), Some(SportyCasual)));
        assert!(!targets(Some("female"), Some(Romantic)));
        assert!(!targets(Some("female"), Some(Edgy)));
        assert!(!targets(None, Some(Street)));
        assert!(!targets(Some("female"), None));
        // 밝은색을 싫어한다고 적은 사용자에게는 켜지 않는다.
        assert!(enabled(Some("male"), Some(Preppy), false));
        assert!(!enabled(Some("male"), Some(Preppy), true));
        assert!(!enabled(Some("male"), Some(Street), false));
    }
}
