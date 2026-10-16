//! 무드를 읽을 수 있는 요소가 하나도 없는 후보를 추천 자리에서 뺀다. 지금은 글램만.
//!
//! 무드는 옷장을 거르는 데만 쓰이고(`clothing_repo::list_clothing_filtered`) 후보를 만드는
//! LLM 에는 전달되지 않는다. 그래서 글램 옷장에 포인트를 받쳐 줄 기본 아이템(화이트 티·
//! 블랙 슬랙스·데님)을 넣으면, 기본 아이템만으로 된 후보도 나온다. 운영 엔진은 그런
//! 후보를 더 높게 친다 — 홈의 '일상' 은 평균 격식 2.5 를 넘기면 감점하고, 아우터 안의
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

/// 이 무드에서 후보가 무드를 읽을 수 있는가. 판단하지 않는 무드는 `None` 이다.
///
/// 글램은 **옷**(상의·하의·아우터)에 글램 요소가 있어야 한다. 예전에는 어느 슬롯이든
/// 하나면 통과해, 기본 티셔츠·데님에 실버 가방만 든 조합이 대표 글램으로 나왔다.
/// 가방·신발의 글램 요소는 옷의 포인트를 거드는 쪽으로만 본다.
pub fn reads_as(mood: StyleGenre, outfit: &[&Clothing]) -> Option<bool> {
    match mood {
        StyleGenre::Glam => Some(
            outfit
                .iter()
                .any(|c| is_garment(&c.category) && is_glam_piece(c)),
        ),
        _ => None,
    }
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
    fn only_glam_is_judged() {
        let tee = item("cotton", "화이트", "cotton jersey");
        let satin = item("satin", "블랙", "satin");
        assert_eq!(reads_as(StyleGenre::Glam, &[&tee]), Some(false));
        assert_eq!(reads_as(StyleGenre::Glam, &[&tee, &satin]), Some(true));
        for g in StyleGenre::ALL.iter().filter(|g| **g != StyleGenre::Glam) {
            assert_eq!(reads_as(*g, &[&tee]), None, "{g}");
        }
    }
}
