//! 무드를 읽을 수 있는 요소가 하나도 없는 후보를 추천 자리에서 뺀다. 글램과 엣지만 본다.
//!
//! 무드는 옷장을 거르고(`clothing_repo::list_clothing_filtered`) 후보 생성 프롬프트에도
//! 전달되지만(`prompts::outfit_candidate_prompts`), 무드 옷장에는 포인트를 받쳐 줄 기본
//! 아이템(화이트 티·블랙 슬랙스·데님)이 함께 있어 기본 아이템만으로 된 후보도 나온다.
//! 운영 엔진은 그런 후보를 더 높게 친다 — 홈의 '일상' 은 평균 격식 2.5 를 넘기면 감점하고, 아우터 안의
//! 강한 상의를 감점한다. 2026-10-16 실제 요청 3회에서 보여 줄 코디 6개 중 3개가 빛을
//! 받는 소재도 조형적인 선도 없는, 무드와 무관한 기본 코디였다.
//!
//! 순위는 건드리지 않는다. 무드 요소가 있는 후보가 하나라도 있으면 없는 후보만 뺀다
//! (`unfit_gate::keep_wearable` 과 같은 방식 — 전부 없으면 그대로 둔다). 다른 무드에는
//! 아무 영향이 없다.
//!
//! 판단은 속성으로만 한다. 글램 요소는 옷(상의·하의·아우터)에 있어야 한다 —
//! 가방·신발만으로는 대표 글램이 되지 않는다.
//!
//! 판단은 속성으로만 한다. 특정 아이템을 지목하지 않고, 메탈릭·미니·힐 같은 한 종류를
//! 강제하지도 않는다 — 소재(새틴·벨벳·메탈릭·퍼·시어링), 실루엣(원숄더·드레이프·비대칭·
//! 아워글라스), 선명한 색 중 하나면 된다. 겨울 코트도 이 기준으로 글램이어야 한다 — 단정한
//! 무광 울 코트는 해당하지 않는다(2026-10-17 사용자).

use crate::models::clothing::Clothing;
use crate::models::style_vocab::StyleGenre;

/// 빛을 받는 소재 또는 조형적인 실루엣이 있는 옷인가.
pub fn is_glam_piece(c: &Clothing) -> bool {
    let material = c.material_primary.as_deref().unwrap_or("");
    if matches!(material, "satin" | "velvet" | "faux fur" | "shearling") {
        return true;
    }
    // 선명한 색 — 광택이나 노출 없이 색으로 시선을 끄는 옷(딥 버건디 롱코트). 옷에만 본다.
    if matches!(c.category.as_str(), "상의" | "하의" | "아우터")
        && c.saturation == Some(crate::models::style_vocab::Saturation::High)
    {
        return true;
    }
    // 색이 곧 금속 광택인 경우. 실버 가죽 가방·실버 샌들.
    if matches!(c.color.as_deref(), Some("실버" | "골드")) {
        return true;
    }
    let keywords = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    // "clean drape" 같은 일반적인 표현에 걸리지 않게 형태를 가리키는 말만 본다.
    [
        "metallic",
        "lurex",
        "sequin",
        "lustrous",
        "one-shoulder",
        "draped",
        "cowl",
        "asymmetric",
        "hourglass",
        "faux fur",
        "shearling",
    ]
    .iter()
    .any(|w| keywords.contains(w))
}

/// 엣지 요소가 있는 옷인가 — 형태(비대칭·각진 어깨·배럴 레그·박시 크롭·크로스오버·
/// 컷아웃)나 가죽 옷(2026-10-17 엣지 무드 추가).
///
/// 옷(상의·하의·아우터)만 본다. 가죽 앵클부츠·가죽 가방은 엣지 코디를 거들 뿐 엣지를
/// 만들지 않는다 — 기본 티·데님에 가죽 가방만 든 조합을 엣지로 내세우지 않는다.
///
/// 검정·오버사이즈 같은 말 하나로는 엣지가 아니다. 헤더그레이 오버사이즈 티, 블랙
/// 그래픽 오버핏 티, "structured, single breasted" 인 단정한 테일러드 재킷은 해당하지
/// 않는다. 특정 아이템 id·이름을 나열하지 않는다.
pub fn is_edgy_piece(c: &Clothing) -> bool {
    has_edgy_shape(c) || is_leather_garment(c)
}

/// 형태 디테일이 있는 옷인가. 소재와 따로 본다 — 비대칭 지퍼의 가죽 재킷도 형태가 있다.
///
/// 형태 단어는 형태를 가리키는 말만 본다 — "structured" 만으로는 세지 않고 "structured
/// shoulder" 처럼 어디가 각졌는지까지 있어야 한다. 드레이프·카울은 글램의 곡선이라 넣지
/// 않는다. 시드 옷은 설명(`texture_keywords`)에, 업로드 옷은 이름에 형태가 적힌다 —
/// 업로드 분석(`clothes::analyze_and_create`)은 설명을 저장하지 않는다.
pub fn has_edgy_shape(c: &Clothing) -> bool {
    if !is_garment(&c.category) {
        return false;
    }
    let keywords = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let shape_in_keywords = [
        "asymmetric",
        "asymmetrical",
        "one-shoulder",
        "barrel",
        "sculpted",
        "structured shoulder",
        "padded shoulder",
        "sharp shoulder",
        "strong shoulder",
        "power shoulder",
        "boxy crop",
        "crossover",
        "cutout",
        "cut-out",
        "deconstructed",
    ]
    .iter()
    .any(|w| keywords.contains(w));
    let shape_in_name = [
        "비대칭",
        "언발란스",
        "원숄더",
        "배럴",
        "컷아웃",
        "크로스오버",
        "파워숄더",
        "스트럭처드 숄더",
    ]
    .iter()
    .any(|w| c.name.contains(w));
    shape_in_keywords || shape_in_name
}

/// 이름에서 옷의 일부만 가죽인 것을 가리키는 말. "레더 트리밍 니트", "가죽 패치 데님",
/// "레더 스트랩 탑" 은 가죽 옷이 아니다.
const PARTIAL_LEATHER_WORDS: [&str; 18] = [
    "트리밍",
    "트림",
    "패치",
    "파이핑",
    "배색",
    "장식",
    "디테일",
    "포인트",
    "벨트",
    "스트랩",
    "버클",
    "칼라",
    "카라",
    "포켓",
    "엘보",
    "단추",
    "버튼",
    "라벨",
];

/// 이름이 말하는 옷의 원단. 업로드 분석은 소재(`material_primary`)를 저장하지 않지만 이름은
/// "색상 + 소재 + 아이템명" 으로 짓는다(`prompts::name_rule`) — 그래서 이름의 소재 단어를
/// 근거로 쓴다.
const FABRIC_NAME_WORDS: [&str; 11] = [
    "니트",
    "데님",
    "울",
    "코튼",
    "저지",
    "린넨",
    "트위드",
    "스웨트",
    "새틴",
    "실크",
    "나일론",
];

/// 확인된 가죽 옷인가. 소재가 저장돼 있으면 소재로만 판단한다(가죽이 아니면 이름에
/// '레더' 가 있어도 아니다). 소재가 비어 있으면 — 업로드 옷이 그렇다 — 이름에 레더·가죽이
/// 있고 트리밍·패치·스트랩처럼 일부만 가죽이라는 말이 없을 때만 가죽 옷이다. 신발·가방은
/// 옷이 아니다. 소재를 지어내지 않는다 — 모르면 가죽이 아니라 "모름" 이다.
pub fn is_leather_garment(c: &Clothing) -> bool {
    if !is_garment(&c.category) {
        return false;
    }
    match c.material_primary.as_deref() {
        Some(m) => m == "leather",
        None => {
            (c.name.contains("레더") || c.name.contains("가죽"))
                && !PARTIAL_LEATHER_WORDS.iter().any(|w| c.name.contains(w))
        }
    }
}

/// 확인된 가죽이 아닌 원단의 옷인가. 저장된 소재가 가죽이 아니거나, 소재가 비어 있으면
/// 이름에 원단(니트·데님·울 등)이 적혀 있고 가죽 옷이 아닐 때다. 소재도 이름의 원단도
/// 없으면 모른다 — 모르는 옷을 가죽과의 대비 근거로 쓰지 않는다.
pub fn is_known_non_leather_garment(c: &Clothing) -> bool {
    if !is_garment(&c.category) {
        return false;
    }
    match c.material_primary.as_deref() {
        Some(m) => m != "leather",
        None => !is_leather_garment(c) && FABRIC_NAME_WORDS.iter().any(|w| c.name.contains(w)),
    }
}

/// 무드 요소를 판단하는 무드인가. 이 무드들은 후보 보완·상담 후보 확장·게이트를 함께 쓴다.
pub fn judges(mood: StyleGenre) -> bool {
    matches!(mood, StyleGenre::Glam | StyleGenre::Edgy | StyleGenre::Pop)
}

/// 선명한 색의 옷인가(팝). 색 이름이 색 계열을 가리키고(`mood_palette::colour_family` — 기본색·
/// 미상은 아니다), 채도가 "낮음" 이 아니어야 한다. 채도가 비어 있으면 이름의 색을 따른다.
/// 차분한 세이지·더스티 핑크처럼 채도가 낮다고 적힌 옷은 팝의 색 대비로 세지 않는다.
pub fn is_vivid_colour(c: &Clothing) -> bool {
    is_garment(&c.category)
        && crate::services::mood_palette::colour_family(c).is_some()
        && c.saturation != Some(crate::models::style_vocab::Saturation::Low)
}

/// 옷 한 벌이 팝의 중심인가 — 색 있는 배색·그래픽·대담한 패턴(2026-10-17 팝 무드 추가).
///
/// 형태를 가리키는 말(배색·컬러블록·그래픽·추상 프린트·굵은 스트라이프·기하·체커보드·멀티컬러)이
/// 있고, 그 옷에 색이 있어야 한다 — 색 이름이 색 계열이거나("코발트/화이트" 배색 포함), 설명에
/// 색 단어가 있다. 흑백 그래픽 티·흑백 체크는 아니다. "striped lining" 처럼 안감·가방의 무늬,
/// 무늬가 없는 단색 옷도 아니다. 시드 밖의 업로드 옷은 설명이 비어 있어 이름의 형태 단어로 본다.
pub fn is_pop_piece(c: &Clothing) -> bool {
    if !is_garment(&c.category) {
        return false;
    }
    // 설명은 쉼표·세미콜론으로 나뉜 구절마다 본다. 안감·트림·파이핑처럼 안쪽이나 가장자리
    // 디테일을 말하는 구절은 뺀다 — "striped lining" 은 무늬 근거가 아니지만, 같은 코트의
    // "bold check in green and red" 구절은 그대로 근거다.
    let keywords = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let outside: Vec<&str> = keywords
        .split([',', ';'])
        .filter(|phrase| !POP_DETAIL_WORDS.iter().any(|w| phrase.contains(w)))
        .collect();
    let pattern_in_keywords = outside
        .iter()
        .any(|phrase| POP_PATTERN_WORDS.iter().any(|w| phrase.contains(w)));
    let pattern_in_name = POP_PATTERN_NAMES.iter().any(|w| c.name.contains(w))
        && !POP_DETAIL_NAMES.iter().any(|w| c.name.contains(w));
    if !(pattern_in_keywords || pattern_in_name) {
        return false;
    }
    let coloured_name = crate::services::mood_palette::colour_family(c).is_some()
        && c.saturation != Some(crate::models::style_vocab::Saturation::Low);
    coloured_name || outside.iter().any(|phrase| has_vivid_colour_word(phrase))
}

/// 팝 요청에서 자동 추천·자동 교체 후보로 쓰지 않는 옷 — 코트를 여미지 않는 기온(12°C 이상)의
/// 겨울 외투(두껍고 보온 안감·충전재가 확인된 아우터). 날씨 판정은 보온 과잉을 보지 않고
/// (`serving_ranker`), 후보 목록은 20°C 부터 무거운 아우터를 거른다 — 19.6°C 실측 6회 중 3회에
/// 다운 패딩이 배색 아우터로 카드에 올랐다(2026-10-17). 홈(`recommendation`)과 상담의 오늘 코디·
/// 말없는 바꾸기(`chat`)가 같은 기준을 쓴다. 팝이 아니거나 기온을 모르면 아무것도 빼지 않는다.
pub fn hidden_on_mild_pop_day(
    c: &Clothing,
    mood: Option<StyleGenre>,
    temperature: Option<f64>,
) -> bool {
    mood == Some(StyleGenre::Pop)
        && temperature.is_some_and(|t| t >= crate::services::serving_ranker::CLOSED_COAT_BELOW_C)
        && crate::services::serving_ranker::is_winter_outer(c)
}

/// 무늬를 가리키는 설명 말.
const POP_PATTERN_WORDS: [&str; 13] = [
    "colour-block",
    "color-block",
    "colorblock",
    "colourblock",
    "graphic print",
    "abstract print",
    "bold stripe",
    "wide stripe",
    "geometric",
    "checkerboard",
    "bold check",
    "multicolour",
    "multicolor",
];

/// 무늬를 가리키는 이름 말. 업로드 옷은 설명이 비어 있어 이름으로 본다.
const POP_PATTERN_NAMES: [&str; 8] = [
    "컬러블록",
    "배색",
    "그래픽",
    "스트라이프",
    "체크",
    "기하",
    "패턴",
    "멀티컬러",
];

/// 옷 겉면이 아닌 안쪽·가장자리 디테일. 이 말이 있는 구절(이름)의 무늬는 근거가 아니다.
const POP_DETAIL_WORDS: [&str; 7] = [
    "lining", "lined", "trim", "piping", "binding", "inside", "interior",
];
const POP_DETAIL_NAMES: [&str; 5] = ["안감", "파이핑", "트리밍", "스티치", "테이프"];

/// 선명한 색 단어. 단어 단위로 본다 — "structured", "tailored" 의 red 는 색이 아니다.
const VIVID_COLOUR_WORDS: [&str; 15] = [
    "red",
    "pink",
    "orange",
    "yellow",
    "green",
    "teal",
    "blue",
    "cobalt",
    "purple",
    "violet",
    "lilac",
    "coral",
    "mustard",
    "tangerine",
    "fuchsia",
];

/// 바로 앞에 오면 색을 흐리게 만드는 말 — "navy blue", "muted green", "dusty pink" 는 선명한 색이
/// 아니다.
const MUTING_WORDS: [&str; 11] = [
    "navy", "muted", "dusty", "faded", "washed", "dull", "pale", "dark", "greyish", "grayish",
    "smoky",
];

fn has_vivid_colour_word(phrase: &str) -> bool {
    let words: Vec<&str> = phrase
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        VIVID_COLOUR_WORDS.contains(w) && !(i > 0 && MUTING_WORDS.contains(&words[i - 1]))
    })
}

/// 이 무드에서 옷 한 벌이 무드 요소인가. 판단하지 않는 무드는 `None` 이다.
pub fn is_mood_piece(mood: StyleGenre, c: &Clothing) -> Option<bool> {
    match mood {
        StyleGenre::Glam => Some(is_glam_piece(c)),
        StyleGenre::Edgy => Some(is_edgy_piece(c)),
        // 팝의 후보 보강·상담 후보 확장용: 배색·그래픽 옷이나 선명한 색 옷. 착장이 팝인지는
        // `reads_as_at` 이 따로 판단한다 — 선명한 옷 한 벌만으로는 팝이 아니다.
        StyleGenre::Pop => Some(is_pop_piece(c) || is_vivid_colour(c)),
        _ => None,
    }
}

/// 이 무드에서 후보가 무드를 읽을 수 있는가. 판단하지 않는 무드는 `None` 이다.
///
/// 글램은 **옷**(상의·하의·아우터)에 글램 요소가 있어야 한다. 예전에는 어느 슬롯이든
/// 하나면 통과해, 기본 티셔츠·데님에 실버 가방만 든 조합이 대표 글램으로 나왔다.
/// 가방·신발의 글램 요소는 옷의 포인트를 거드는 쪽으로만 본다.
///
/// 엣지도 같다 — 형태나 가죽이 옷에 있어야 한다. 가죽 부츠·가방은 세지 않는다.
// 앱 경로는 기온을 아는 `reads_as_at` 을 쓴다. 이 함수는 라이브러리의 테스트·예제가 쓰는데,
// 같은 모듈이 바이너리에도 컴파일되어 거기서는 쓰이지 않는다.
#[allow(dead_code)]
pub fn reads_as(mood: StyleGenre, outfit: &[&Clothing]) -> Option<bool> {
    reads_as_at(mood, outfit, None)
}

/// 기온을 아는 판정. 팝만 기온을 쓴다 — 코트를 여며 입는 기온(`CLOSED_COAT_BELOW_C` 미만)에는
/// 아우터가 있으면 코트 속 상의의 색·그래픽을 근거로 세지 않는다. 다른 무드는 `reads_as` 와 같다.
///
/// 팝: 보이는 옷(상의·하의·아우터)에 색 있는 배색·그래픽 옷이 있거나(`is_pop_piece`), 서로 다른 색
/// 계열의 선명한 옷이 둘 이상이어야 한다(`is_vivid_colour`). 선명한 옷 한 벌에 기본 데님, 가방·
/// 신발만 유색인 조합은 아니다.
pub fn reads_as_at(
    mood: StyleGenre,
    outfit: &[&Clothing],
    temperature: Option<f64>,
) -> Option<bool> {
    if !judges(mood) {
        return None;
    }
    if mood == StyleGenre::Pop {
        let has_outer = outfit.iter().any(|c| c.category == "아우터");
        let top_hidden = has_outer
            && temperature
                .is_some_and(|t| t < crate::services::serving_ranker::CLOSED_COAT_BELOW_C);
        let visible: Vec<&&Clothing> = outfit
            .iter()
            .filter(|c| is_garment(&c.category) && !(top_hidden && c.category == "상의"))
            .collect();
        if visible.iter().any(|c| is_pop_piece(c)) {
            return Some(true);
        }
        let families: std::collections::HashSet<&str> = visible
            .iter()
            .filter(|c| is_vivid_colour(c))
            .filter_map(|c| crate::services::mood_palette::colour_family(c))
            .collect();
        return Some(families.len() >= 2);
    }
    Some(
        outfit
            .iter()
            .any(|c| is_garment(&c.category) && is_mood_piece(mood, c) == Some(true)),
    )
}

fn is_garment(category: &str) -> bool {
    matches!(category, "상의" | "하의" | "아우터")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(material: &str, color: &str, keywords: &str) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: "x".into(),
            name: "x".into(),
            category: "상의".into(),
            color: Some(color.into()),
            thickness: crate::models::style_vocab::Thickness::Thin,
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
            material_primary: Some(material.into()),
            sub_category: None,
            floating_score: None,
            strong_style_score: None,
            texture_keywords: Some(keywords.into()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn light_catching_fabric_or_sculpted_line_counts() {
        assert!(is_glam_piece(&item("satin", "블랙", "satin, midi length")));
        assert!(is_glam_piece(&item("velvet", "버건디", "stretch velvet")));
        assert!(is_glam_piece(&item(
            "knit",
            "샴페인",
            "fine lurex knit, subtle metallic sheen"
        )));
        assert!(is_glam_piece(&item(
            "jersey",
            "블랙",
            "matte stretch jersey, one-shoulder neckline"
        )));
        assert!(is_glam_piece(&item(
            "leather",
            "실버",
            "leather, silver chain, mini"
        )));
    }

    /// 기본 아이템은 글램 요소가 아니다. "clean drape" 처럼 원단이 떨어지는 모양을
    /// 말하는 표현은 드레이프 디자인이 아니다.
    #[test]
    fn plain_basics_do_not_count() {
        assert!(!is_glam_piece(&item(
            "cotton",
            "화이트",
            "cotton jersey, relaxed crewneck"
        )));
        assert!(!is_glam_piece(&item(
            "polyester",
            "블랙",
            "wide leg, clean drape"
        )));
        assert!(!is_glam_piece(&item(
            "denim",
            "다크블루",
            "washed denim, dark, wide leg"
        )));
        assert!(!is_glam_piece(&item(
            "leather",
            "블랙",
            "leather, block heel ankle boot"
        )));
    }

    /// 기본 티셔츠·데님에 실버 가방만 더한 조합은 글램으로 치지 않는다.
    #[test]
    fn a_silver_bag_alone_does_not_make_it_glam() {
        let tee = item("cotton", "화이트", "cotton jersey");
        let mut denim = item("denim", "다크블루", "washed denim");
        denim.category = "하의".into();
        let mut bag = item("leather", "실버", "leather, silver chain, mini");
        bag.category = "가방".into();
        assert!(is_glam_piece(&bag));
        assert_eq!(
            reads_as(StyleGenre::Glam, &[&tee, &denim, &bag]),
            Some(false)
        );
        let mut skirt = item("satin", "블랙", "satin, midi");
        skirt.category = "하의".into();
        assert_eq!(
            reads_as(StyleGenre::Glam, &[&tee, &skirt, &bag]),
            Some(true)
        );
    }

    #[test]
    fn only_glam_and_edgy_are_judged() {
        let tee = item("cotton", "화이트", "cotton jersey");
        let satin = item("satin", "블랙", "satin");
        assert_eq!(reads_as(StyleGenre::Glam, &[&tee]), Some(false));
        assert_eq!(reads_as(StyleGenre::Glam, &[&tee, &satin]), Some(true));
        assert_eq!(reads_as(StyleGenre::Edgy, &[&tee]), Some(false));
        assert_eq!(reads_as(StyleGenre::Pop, &[&tee]), Some(false));
        for g in StyleGenre::ALL
            .iter()
            .filter(|g| !matches!(g, StyleGenre::Glam | StyleGenre::Edgy | StyleGenre::Pop))
        {
            assert_eq!(reads_as(*g, &[&tee]), None, "{g}");
        }
    }

    fn with(mut c: Clothing, name: &str, category: &str, sub: &str) -> Clothing {
        c.name = name.into();
        c.category = category.into();
        c.sub_category = Some(sub.into());
        c
    }

    /// 엣지 요소는 옷의 형태나 가죽 옷이다. 시드 밖의 옷(이름·설명이 다른 옷)도 같은
    /// 기준으로 걸린다.
    #[test]
    fn edgy_reads_shape_or_a_leather_garment() {
        for (name, cat, material, kw) in [
            (
                "차콜 비대칭 헴 저지 롱슬리브",
                "상의",
                "jersey",
                "asymmetric hem, long sleeves",
            ),
            (
                "블랙 스트럭처드 숄더 하이넥 탑",
                "상의",
                "jersey",
                "structured padded shoulders, high neck",
            ),
            (
                "차콜 울 배럴 레그 팬츠",
                "하의",
                "wool",
                "curved barrel leg, front pleats",
            ),
            (
                "블랙 레더 미디 스커트",
                "하의",
                "leather",
                "smooth matte leather, midi",
            ),
            (
                "블랙 박시 크롭 울 재킷",
                "아우터",
                "wool",
                "boxy cropped cut, sculpted shoulders",
            ),
            (
                "차콜 울 오버사이즈 블레이저",
                "아우터",
                "wool",
                "wool flannel, structured shoulder, oversized blazer",
            ),
            // 시드 밖: 업로드 옷처럼 설명이 비어 있고 이름에 형태가 있다.
            ("네이비 언발란스 헴 니트", "상의", "knit", ""),
            ("블랙 컷아웃 숄더 롱슬리브", "상의", "cotton", ""),
            // 시드 밖: 설명에만 형태가 있다.
            (
                "아이보리 크로스오버 넥 탑",
                "상의",
                "viscose",
                "crossover neckline, fitted",
            ),
        ] {
            let c = with(item(material, "블랙", kw), name, cat, "x");
            assert!(is_edgy_piece(&c), "{name}");
        }
    }

    /// 검정·오버사이즈·"structured" 같은 말 하나, 글램의 드레이프, 가죽 신발·가방은
    /// 엣지 요소가 아니다. 실제 옷장에 있는 기본 아이템들이다.
    #[test]
    fn plain_or_accessory_leather_is_not_edgy() {
        for (name, cat, material, kw) in [
            (
                "헤더그레이 오버사이즈 티셔츠",
                "상의",
                "cotton",
                "cotton jersey, oversized, dropped shoulder",
            ),
            (
                "블랙 그래픽 오버핏 티셔츠",
                "상의",
                "cotton",
                "cotton, graphic print, oversized",
            ),
            (
                "블랙 싱글 테일러드 자켓",
                "아우터",
                "polyester",
                "structured, single breasted",
            ),
            (
                "블랙 와이드 슬랙스",
                "하의",
                "polyester",
                "wide leg, clean drape",
            ),
            (
                "에메랄드 새틴 드레이프 블라우스",
                "상의",
                "satin",
                "fluid satin, draped cowl neckline",
            ),
            (
                "블랙 메쉬 레이어드 롱슬리브",
                "상의",
                "polyester",
                "mesh layered, see-through",
            ),
            (
                "워시드 블랙 슬림 데님",
                "하의",
                "denim",
                "denim, slim leg, washed black",
            ),
            (
                "블랙 레더 앵클부츠",
                "신발",
                "leather",
                "leather, block heel ankle boot",
            ),
            (
                "블랙 레더 미니 숄더백",
                "가방",
                "leather",
                "leather, structured mini",
            ),
            // 이름에 가죽 장식이 있어도 소재가 가죽이 아니면 가죽 옷이 아니다.
            (
                "블랙 레더 트리밍 니트",
                "상의",
                "knit",
                "knit, faux leather trim at the cuffs",
            ),
        ] {
            let c = with(item(material, "블랙", kw), name, cat, "x");
            assert!(!is_edgy_piece(&c), "{name}");
        }
    }

    /// 업로드 옷처럼 소재가 비어 있으면 이름으로 본다. 가죽 옷만 가죽이고, 일부만 가죽인
    /// 옷(트리밍·패치·스트랩·포인트)과 신발·가방은 아니다. 소재가 저장돼 있으면 이름보다
    /// 소재가 우선이다. 소재도 이름의 원단도 없으면 "모름" 이다.
    #[test]
    fn missing_material_falls_back_to_the_name_without_guessing() {
        let unknown = |name: &str, cat: &str| {
            let mut c = with(item("", "블랙", ""), name, cat, "");
            c.material_primary = None;
            c.texture_keywords = None;
            c.sub_category = None;
            c
        };
        for (name, cat) in [
            ("블랙 레더 미디 스커트", "하의"),
            ("브라운 가죽 라이더 자켓", "아우터"),
            ("블랙 페이크 레더 와이드 팬츠", "하의"),
        ] {
            let c = unknown(name, cat);
            assert!(is_leather_garment(&c), "{name}");
            assert!(is_edgy_piece(&c), "{name}");
        }
        for (name, cat) in [
            ("블랙 레더 트리밍 니트", "상의"),
            ("인디고 가죽 패치 데님", "하의"),
            ("블랙 레더 스트랩 탑", "상의"),
            ("그레이 레더 포인트 가디건", "아우터"),
            ("블랙 레더 앵클부츠", "신발"),
            ("블랙 레더 숄더백", "가방"),
        ] {
            assert!(!is_leather_garment(&unknown(name, cat)), "{name}");
        }
        // 소재가 저장돼 있으면 소재가 우선이다.
        let mut knit = unknown("블랙 레더룩 니트", "상의");
        knit.material_primary = Some("knit".into());
        assert!(!is_leather_garment(&knit));
        assert!(is_known_non_leather_garment(&knit));
        // 원단을 말하는 이름은 비가죽 근거, 아무 말도 없으면 모름.
        assert!(is_known_non_leather_garment(&unknown(
            "블랙 리브드 니트",
            "상의"
        )));
        assert!(!is_known_non_leather_garment(&unknown(
            "블랙 기본 탑",
            "상의"
        )));
        assert!(!is_known_non_leather_garment(&unknown(
            "블랙 레더 미디 스커트",
            "하의"
        )));
    }

    /// 형태는 소재와 따로 본다 — 비대칭 지퍼의 가죽 재킷은 형태가 있고, 기본 라이더는 없다.
    #[test]
    fn shape_is_judged_apart_from_leather() {
        let mut zip = with(
            item("leather", "블랙", "smooth leather, asymmetric zip front"),
            "블랙 레더 재킷",
            "아우터",
            "x",
        );
        assert!(has_edgy_shape(&zip));
        zip.texture_keywords = Some("leather, cropped biker, silver hardware".into());
        assert!(!has_edgy_shape(&zip));
        assert!(is_leather_garment(&zip));
    }

    /// 기본 티·데님에 가죽 가방·부츠만 더한 조합은 엣지가 아니다. 같은 조합에서 하의만
    /// 가죽 스커트로 바꾸거나, 가죽 라이더를 걸치면 엣지다.
    #[test]
    fn leather_accessories_alone_do_not_make_it_edgy() {
        let tee = with(
            item("cotton", "화이트", "cotton jersey, relaxed crewneck"),
            "화이트 티",
            "상의",
            "x",
        );
        let denim = with(
            item("denim", "인디고", "denim, wide leg"),
            "인디고 와이드 데님",
            "하의",
            "x",
        );
        let boots = with(
            item("leather", "블랙", "lace-up, lug sole"),
            "블랙 컴뱃 부츠",
            "신발",
            "앵클부츠",
        );
        let bag = with(
            item("leather", "블랙", "angular trapezoid shape"),
            "블랙 숄더백",
            "가방",
            "숄더백",
        );
        assert_eq!(
            reads_as(StyleGenre::Edgy, &[&tee, &denim, &boots, &bag]),
            Some(false)
        );
        let skirt = with(
            item("leather", "블랙", "matte leather, midi"),
            "블랙 레더 미디 스커트",
            "하의",
            "스커트",
        );
        assert_eq!(
            reads_as(StyleGenre::Edgy, &[&tee, &skirt, &boots, &bag]),
            Some(true)
        );
        let rider = with(
            item("leather", "블랙", "cropped biker"),
            "블랙 레더 라이더 자켓",
            "아우터",
            "라이더자켓",
        );
        assert_eq!(
            reads_as(StyleGenre::Edgy, &[&tee, &denim, &rider, &bag]),
            Some(true)
        );
    }

    fn garment(
        name: &str,
        cat: &str,
        color: &str,
        sat: Option<crate::models::style_vocab::Saturation>,
        kw: &str,
    ) -> Clothing {
        let mut c = with(item("cotton", color, kw), name, cat, "x");
        c.saturation = sat;
        c
    }

    use crate::models::style_vocab::Saturation::{High, Low, Mid};

    /// 레이스 트리밍·셔링만으로는 글램도 엣지도 아니다 — 부드러운 장식은 로맨틱의 질감이다.
    /// 같은 셔링 저지라도 비대칭 랩넥이면 글램 형태가 있다. 밝은 색 긴팔도 절개 구조
    /// (deconstructed)가 실제로 있으면 엣지다 — 색이 아니라 구조로 본다. 일부에만 새틴이 붙은
    /// 무광 저지 탑은 옷 전체가 빛을 받는 소재가 아니라 글램 옷으로 세지 않는다(2026-10-18 사진 참고).
    #[test]
    fn lace_and_ruching_alone_open_neither_glam_nor_edgy() {
        let mut lace = garment(
            "라이트그레이 레이스 트리밍 셔링 티",
            "상의",
            "라이트그레이",
            Some(Low),
            "soft jersey, lace-trimmed neckline, side ruching, long sleeves",
        );
        lace.material_primary = Some("jersey".into());
        assert!(!is_glam_piece(&lace));
        assert!(!is_edgy_piece(&lace));

        let mut wrap = lace.clone();
        wrap.texture_keywords =
            Some("soft jersey, asymmetric wrap neckline edged with lace, side ruching".into());
        assert!(is_glam_piece(&wrap));

        let mut slashed = garment(
            "화이트 저지 롱슬리브",
            "상의",
            "화이트",
            Some(Low),
            "fine jersey, one-piece top with a deconstructed front of slashed strips over an attached opaque inner panel",
        );
        slashed.material_primary = Some("jersey".into());
        assert!(is_edgy_piece(&slashed));
        let mut plain_white = slashed.clone();
        plain_white.texture_keywords = Some("fine jersey, scoop neck, long sleeves".into());
        assert!(!is_edgy_piece(&plain_white));

        let mut band = garment(
            "그레이 셔링 튜브탑",
            "상의",
            "그레이/퍼플",
            Some(Low),
            "ruched grey stretch mesh over an opaque lining, attached satin band edged with lace",
        );
        band.material_primary = Some("jersey".into());
        assert!(!is_glam_piece(&band));
    }

    /// 팝 한 벌의 근거: 색 있는 배색·그래픽·대담한 패턴. 흑백 그래픽, 안감 무늬, 무늬 없는 단색,
    /// 가방은 아니다. 업로드 옷처럼 설명이 비어도 이름의 형태 단어와 색으로 본다.
    #[test]
    fn a_pop_piece_needs_a_coloured_pattern() {
        for c in [
            garment(
                "코발트 컬러블록 니트",
                "상의",
                "코발트/화이트",
                Some(High),
                "colour-block knit, cobalt and white panels",
            ),
            garment(
                "멀티 스트라이프 셔츠",
                "상의",
                "화이트",
                Some(Low),
                "cotton poplin, bold stripe in red and blue",
            ),
            garment("레드 체크 스커트", "하의", "레드", None, ""), // 업로드 옷: 설명 없음
            garment("그린 기하 패턴 재킷", "아우터", "그린/핑크", Some(Mid), ""),
            // 겉감 체크 구절은 근거다 — 같은 설명에 보온 안감이 있어도.
            garment(
                "체크 울 코트",
                "아우터",
                "그린/레드",
                Some(High),
                "heavy wool, bold check in green and red, insulated lining",
            ),
            // 바탕이 흰색이어도 실제 색 그래픽이 있으면 근거다.
            garment(
                "화이트 그래픽 티",
                "상의",
                "화이트",
                Some(Low),
                "cotton jersey, abstract print in cobalt blue and pink",
            ),
        ] {
            assert!(is_pop_piece(&c), "{}", c.name);
        }
        for c in [
            garment(
                "블랙 그래픽 오버핏 티셔츠",
                "상의",
                "블랙",
                Some(Low),
                "cotton, graphic print, oversized",
            ),
            garment(
                "네이비 스트라이프 럭비 티",
                "상의",
                "네이비",
                Some(Low),
                "cotton, rugby stripe",
            ),
            garment(
                "블랙 체커보드 니트",
                "상의",
                "블랙/화이트",
                Some(Low),
                "checkerboard knit in black and white",
            ),
            garment(
                "베이지 트렌치",
                "아우터",
                "베이지",
                Some(Low),
                "cotton twill, plain striped lining",
            ),
            garment(
                "체리레드 니트",
                "상의",
                "체리레드",
                Some(High),
                "fine knit, crew neck",
            ),
            // "structured"·"tailored" 의 red 는 색이 아니다.
            garment(
                "차콜 기하 자카드 재킷",
                "아우터",
                "차콜",
                Some(Low),
                "structured tailored jacket, geometric jacquard",
            ),
            // 흐린 색 단어는 선명한 색이 아니다.
            garment(
                "그레이 스트라이프 셔츠",
                "상의",
                "그레이",
                Some(Low),
                "bold stripe in navy blue and muted green",
            ),
            // 무늬가 안감·트림에만 있다.
            garment(
                "블랙 울 코트",
                "아우터",
                "블랙",
                Some(Low),
                "wool, red and blue geometric lining",
            ),
            garment("네이비 배색 파이핑 셔츠", "상의", "네이비", None, ""),
            garment(
                "멀티컬러 그래픽 토트백",
                "가방",
                "레드",
                Some(High),
                "graphic print canvas",
            ),
        ] {
            assert!(!is_pop_piece(&c), "{}", c.name);
        }
    }

    /// 착장: 배색·그래픽 옷 한 벌이거나, 서로 다른 색 계열의 선명한 옷 둘. 선명한 옷 한 벌에 기본
    /// 데님, 같은 계열 둘, 차분한(채도 낮음) 색, 가방·신발 색은 아니다.
    #[test]
    fn pop_reads_from_a_pattern_or_two_vivid_colours() {
        let cobalt = garment("코발트 니트", "상의", "코발트", Some(High), "fine knit");
        let red_skirt = garment(
            "체리레드 스커트",
            "하의",
            "체리레드",
            Some(High),
            "a-line skirt",
        );
        let red_top = garment("레드 티", "상의", "레드", Some(High), "cotton jersey");
        let denim = garment("인디고 데님", "하의", "인디고", Some(Low), "denim");
        let sage = garment(
            "세이지 스커트",
            "하의",
            "세이지그린",
            Some(Low),
            "matte skirt",
        );
        let mut bag = garment("옐로 숄더백", "가방", "옐로", Some(High), "leather");
        bag.category = "가방".into();
        let graphic = garment(
            "멀티 그래픽 티",
            "상의",
            "화이트/레드",
            Some(Mid),
            "abstract print in red and blue",
        );
        let pop = |o: &[&Clothing]| reads_as(StyleGenre::Pop, o);
        assert_eq!(pop(&[&cobalt, &red_skirt]), Some(true));
        assert_eq!(pop(&[&graphic, &denim]), Some(true));
        assert_eq!(pop(&[&red_top, &denim, &bag]), Some(false));
        assert_eq!(pop(&[&red_top, &red_skirt]), Some(false));
        assert_eq!(pop(&[&cobalt, &sage]), Some(false));
    }

    /// 코트를 여미는 기온에는 코트 속 상의의 색·그래픽을 세지 않는다. 배색 코트면 팝이다.
    /// 온화한 날 팝 후보에서 빠지는 것은 겨울 외투뿐이다. 찬 날·다른 무드·기온 미상은 그대로다.
    #[test]
    fn only_winter_outers_are_hidden_on_mild_pop_days() {
        let mut puffer = garment(
            "핑크 오렌지 컬러블록 패딩",
            "아우터",
            "핑크/오렌지",
            Some(High),
            "colour-block puffer, down fill, hip-length",
        );
        puffer.thickness = crate::models::style_vocab::Thickness::Thick;
        let windbreaker = garment(
            "코발트 옐로 컬러블록 바람막이",
            "아우터",
            "코발트/옐로",
            Some(High),
            "lightweight nylon, unlined",
        );
        let pop = Some(StyleGenre::Pop);
        assert!(hidden_on_mild_pop_day(&puffer, pop, Some(19.6)));
        assert!(hidden_on_mild_pop_day(&puffer, pop, Some(12.0)));
        for t in [Some(8.0), Some(2.0), None] {
            assert!(!hidden_on_mild_pop_day(&puffer, pop, t), "{t:?}");
        }
        assert!(!hidden_on_mild_pop_day(&windbreaker, pop, Some(19.6)));
        for m in [Some(StyleGenre::Edgy), Some(StyleGenre::Street), None] {
            assert!(!hidden_on_mild_pop_day(&puffer, m, Some(19.6)));
        }
    }

    #[test]
    fn a_top_under_a_closed_coat_does_not_make_it_pop() {
        let graphic = garment(
            "멀티 그래픽 티",
            "상의",
            "화이트/레드",
            Some(Mid),
            "abstract print in red and blue",
        );
        let denim = garment("인디고 데님", "하의", "인디고", Some(Low), "denim");
        let navy_coat = garment("네이비 울 코트", "아우터", "네이비", Some(Low), "wool coat");
        let block_coat = garment(
            "컬러블록 울 코트",
            "아우터",
            "코발트/오렌지",
            Some(High),
            "colour-block wool coat, insulated lining",
        );
        let at = |o: &[&Clothing], t| reads_as_at(StyleGenre::Pop, o, Some(t));
        assert_eq!(at(&[&graphic, &denim, &navy_coat], 16.0), Some(true));
        assert_eq!(at(&[&graphic, &denim, &navy_coat], 5.0), Some(false));
        assert_eq!(at(&[&graphic, &denim, &block_coat], 5.0), Some(true));
        assert_eq!(at(&[&graphic, &denim], 5.0), Some(true));
    }
}
