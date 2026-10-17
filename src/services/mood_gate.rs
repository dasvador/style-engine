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
    matches!(mood, StyleGenre::Glam | StyleGenre::Edgy)
}

/// 이 무드에서 옷 한 벌이 무드 요소인가. 판단하지 않는 무드는 `None` 이다.
pub fn is_mood_piece(mood: StyleGenre, c: &Clothing) -> Option<bool> {
    match mood {
        StyleGenre::Glam => Some(is_glam_piece(c)),
        StyleGenre::Edgy => Some(is_edgy_piece(c)),
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
pub fn reads_as(mood: StyleGenre, outfit: &[&Clothing]) -> Option<bool> {
    if !judges(mood) {
        return None;
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
        for g in StyleGenre::ALL
            .iter()
            .filter(|g| !matches!(g, StyleGenre::Glam | StyleGenre::Edgy))
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
}
