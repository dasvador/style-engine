//! 색 다양성 성별·무드의 후보 목록(`shortlist::build_all_shortlists_with_palette`).
//!
//! 기본 목록을 만든 뒤 상의·아우터에서 빠진 색 계열을 계열마다 한 벌씩(최대 3) 더한다. 날씨에
//! 맞지 않는 옷은 더하지 않고, 역할의 유일한 대표와 색 계열의 유일한 옷은 빼지 않는다. 시드
//! 밖의 옷, 파스텔, 색 이름 미상, 컬러 없는 옷장, 같은 계열 반복을 반례로 둔다. DB·LLM 을 쓰지
//! 않는다.

use std::collections::HashSet;

use style_engine::models::clothing::Clothing;
use style_engine::models::style_vocab::{Role, Saturation, Style, StyleGenre, Thickness};
use style_engine::services::mood_palette::colour_family;
use style_engine::services::shortlist::{
    ShortlistContext, build_all_shortlists, build_all_shortlists_with_palette,
};

fn item(id: &str, category: &str, color: &str, role: Role, style: Style) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: id.into(),
        name: format!("{color} {id}"),
        category: category.into(),
        color: (!color.is_empty()).then(|| color.into()),
        thickness: Thickness::Medium,
        image_url: None,
        tone: None,
        saturation: Some(Saturation::Low),
        style: Some(style),
        weight: None,
        role: Some(role),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(2),
        gender: Some("male".into()),
        style_mood: Some(StyleGenre::Amekaji),
        visual_weight: None,
        texture_depth: None,
        visual_weight_v2: None,
        texture_depth_v2: None,
        grounding_score: None,
        shadow_tone: None,
        silhouette_volume: None,
        material_primary: Some("cotton".into()),
        sub_category: None,
        floating_score: None,
        strong_style_score: None,
        texture_keywords: None,
        created_at: now,
        updated_at: now,
    }
}

/// 기본색 베이스 상의 13벌(목록 12칸을 넘친다) + 점수가 낮은 컬러 셔츠들(포인트·워크).
fn wardrobe() -> Vec<Clothing> {
    let mut v: Vec<Clothing> = (0..13)
        .map(|i| {
            let color = ["블랙", "화이트", "그레이", "네이비", "베이지"][i % 5];
            item(
                &format!("basic{i}"),
                "상의",
                color,
                Role::Base,
                Style::Basic,
            )
        })
        .collect();
    v.push(item("brick", "상의", "브릭레드", Role::Accent, Style::Work));
    v.push(item("royal", "상의", "로얄블루", Role::Accent, Style::Work));
    v.push(item("sky", "상의", "하늘색", Role::Accent, Style::Work)); // 같은 블루 계열
    v.push(item(
        "mustard",
        "상의",
        "머스타드",
        Role::Accent,
        Style::Work,
    )); // 시드 밖의 색
    v.push(item(
        "lavender",
        "상의",
        "라벤더",
        Role::Accent,
        Style::Work,
    )); // 파스텔
    v.push(item("unknown", "상의", "미상", Role::Accent, Style::Work)); // 색 이름 미상
    v
}

fn ctx(temp: f64, none: &HashSet<String>) -> ShortlistContext<'_> {
    ShortlistContext {
        temperature: temp,
        situation: Some("일상"),
        current_season: Some("가을"),
        recent_item_ids: none,
    }
}

/// 기본 목록에서는 점수가 낮은 컬러 셔츠가 잘린다. 색 계열 목록은 빠진 계열을 계열마다 한
/// 벌씩, 최대 세 계열을 더한다 — 같은 블루 계열 둘은 한 벌만, 색 이름 미상은 넣지 않는다.
/// 기본색 옷이 여전히 대부분이다.
#[test]
fn missing_colour_families_survive_the_cut() {
    let none = HashSet::new();
    let w = wardrobe();
    let base = build_all_shortlists(&w, &ctx(19.6, &none));
    let pal = build_all_shortlists_with_palette(&w, &ctx(19.6, &none));
    let fams = |v: &[&Clothing]| -> Vec<&'static str> {
        v.iter().filter_map(|c| colour_family(c)).collect()
    };
    let base_fams = fams(&base.tops);
    let pal_fams = fams(&pal.tops);
    assert!(base_fams.len() <= 1, "{base_fams:?}");
    let unique: HashSet<_> = pal_fams.iter().collect();
    assert_eq!(
        unique.len(),
        pal_fams.len(),
        "같은 계열이 두 번 들어갔다: {pal_fams:?}"
    );
    assert!(
        pal_fams.len() >= 3 && pal_fams.len() <= base_fams.len() + 3,
        "{pal_fams:?}"
    );
    assert_eq!(pal.tops.len(), base.tops.len());
    assert!(!pal.tops.iter().any(|c| c.id == "unknown"));
    let basics = pal
        .tops
        .iter()
        .filter(|c| colour_family(c).is_none())
        .count();
    assert!(basics >= pal.tops.len() - 4, "기본색이 밀려났다: {basics}");
}

/// 컬러 옷이 없는 옷장에서는 기본 목록과 같다.
#[test]
fn a_wardrobe_without_colour_is_unchanged() {
    let none = HashSet::new();
    let w: Vec<Clothing> = wardrobe()
        .into_iter()
        .filter(|c| c.id.starts_with("basic"))
        .collect();
    let ids = |v: &[&Clothing]| v.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(
        ids(&build_all_shortlists(&w, &ctx(19.6, &none)).tops),
        ids(&build_all_shortlists_with_palette(&w, &ctx(19.6, &none)).tops)
    );
}

/// 날씨에 맞지 않는 옷은 색이 있어도 더하지 않는다 — 26°C 의 두꺼운 컬러 패딩, 무거운 아우터.
#[test]
fn colour_does_not_bypass_the_weather_filter() {
    let none = HashSet::new();
    let mut w = wardrobe();
    let mut padding = item("padding", "아우터", "버건디", Role::Accent, Style::Basic);
    padding.thickness = Thickness::Thick;
    padding.weight = Some(style_engine::models::style_vocab::Weight::Heavy);
    padding.name = "버건디 퀼팅 숏 패딩".into();
    w.push(padding);
    w.push(item(
        "coat",
        "아우터",
        "블랙",
        Role::Structural,
        Style::Basic,
    ));
    let hot = build_all_shortlists_with_palette(&w, &ctx(26.0, &none));
    assert!(!hot.outers.iter().any(|c| c.id == "padding"));
    let cold = build_all_shortlists_with_palette(&w, &ctx(5.0, &none));
    assert!(cold.outers.iter().any(|c| c.id == "padding"));
}

/// 역할의 유일한 대표는 빼지 않는다 — 구조템 상의가 하나뿐이면 컬러가 들어와도 남는다.
#[test]
fn the_only_item_of_a_role_is_kept() {
    let none = HashSet::new();
    let mut w = wardrobe();
    w.push(item(
        "structured",
        "상의",
        "차콜",
        Role::Structural,
        Style::Basic,
    ));
    let pal = build_all_shortlists_with_palette(&w, &ctx(19.6, &none));
    assert!(pal.tops.iter().any(|c| c.id == "structured"));
}

/// 재현: 기본 목록에는 이름이 깨진 블루 옷(점수 높은 베이스)이 남고, 정상 이름의 블루 옷(점수
/// 낮은 포인트)은 잘린다. 색 다양성 목록은 깨진 옷을 빼고, 블루 계열을 정상 이름의 옷으로
/// 보완하며, 그 옷이 모델에 보내는 색 목록에 실린다. 깨진 옷은 다른 슬롯에서도 빠진다.
#[test]
fn a_broken_name_neither_reaches_the_model_nor_blocks_its_colour() {
    let none = HashSet::new();
    // 기본색 베이스 11벌 + 깨진 블루(베이스·베이직) = 목록 12칸. 정상 블루는 같은 베이스 역할의
    // 워크 스타일이라 점수가 낮아 13번째로 잘린다.
    let mut w: Vec<Clothing> = wardrobe()
        .into_iter()
        .filter(|c| c.id.starts_with("basic"))
        .take(11)
        .collect();
    let mut broken = item("rugby", "상의", "슬레이트블루", Role::Base, Style::Basic);
    broken.name = "슬\u{FFFD}\u{FFFD}이트블루 럭비 셔츠".into();
    w.push(broken);
    w.push(item("royal", "상의", "로얄블루", Role::Base, Style::Work));
    let mut broken_boots = item("boots", "신발", "샌드", Role::Base, Style::Basic);
    broken_boots.name = "샌드 스웨이드 데저\u{FFFD}\u{FFFD}부츠".into();
    w.push(broken_boots);
    w.push(item("sneaker", "신발", "화이트", Role::Base, Style::Basic));

    let base = build_all_shortlists(&w, &ctx(19.6, &none));
    assert!(
        base.tops.iter().any(|c| c.id == "rugby"),
        "재현 조건: 깨진 블루가 기본 목록에 남는다"
    );
    assert!(
        !base.tops.iter().any(|c| c.id == "royal"),
        "재현 조건: 정상 블루는 잘린다"
    );

    let pal = build_all_shortlists_with_palette(&w, &ctx(19.6, &none));
    assert!(!pal.tops.iter().any(|c| c.id == "rugby"));
    assert!(pal.tops.iter().any(|c| c.id == "royal"));
    assert!(
        pal.colour_items()
            .iter()
            .any(|n| n.contains("로얄블루") && n.contains("(블루)"))
    );
    assert!(pal.colour_items().iter().all(|n| !n.contains('\u{FFFD}')));
    assert!(!pal.shoes.iter().any(|c| c.id == "boots"));
    assert!(pal.shoes.iter().any(|c| c.id == "sneaker"));
    assert_eq!(pal.tops.len(), base.tops.len());
}

/// 팝 후보 지시의 옷 목록: 색 있는 배색·그래픽 옷은 "(배색·그래픽)", 선명한 단색 옷은 색 계열로
/// 적는다. 흑백 무늬·흐린 색·기본색·신발·가방·깨진 이름은 넣지 않는다. 시드 id·이름 규칙이 아닌
/// 속성으로 본다(업로드 옷 이름 "레드 체크 스커트").
#[test]
fn pop_items_name_coloured_patterns_and_vivid_garments_only() {
    let none = HashSet::new();
    let mut w = wardrobe()
        .into_iter()
        .filter(|c| c.id.starts_with("basic"))
        .take(6)
        .collect::<Vec<_>>();
    let vivid = |id: &str, category: &str, color: &str, sat: Saturation, kw: &str| {
        let mut c = item(id, category, color, Role::Accent, Style::Basic);
        c.saturation = Some(sat);
        c.texture_keywords = (!kw.is_empty()).then(|| kw.into());
        c
    };
    w.push(vivid(
        "knit",
        "상의",
        "코발트",
        Saturation::High,
        "soft knit",
    ));
    w.push(vivid("check", "하의", "레드", Saturation::High, ""));
    w.last_mut().unwrap().name = "레드 체크 스커트".into();
    w.push(vivid(
        "mono",
        "상의",
        "블랙/화이트",
        Saturation::Low,
        "checkerboard knit in black and white",
    ));
    w.push(vivid(
        "muted",
        "하의",
        "세이지",
        Saturation::Low,
        "cotton twill",
    ));
    w.push(vivid(
        "bag",
        "가방",
        "레드",
        Saturation::High,
        "graphic print",
    ));
    let mut broken = vivid("broken", "상의", "핑크", Saturation::High, "");
    broken.name = "핑\u{FFFD}\u{FFFD} 니트".into();
    w.push(broken);

    let got = build_all_shortlists_with_palette(&w, &ctx(19.6, &none)).pop_items();
    assert!(got.contains(&"코발트 knit(블루)".to_string()), "{got:?}");
    assert!(
        got.contains(&"레드 체크 스커트(배색·그래픽)".to_string()),
        "{got:?}"
    );
    assert_eq!(got.len(), 2, "{got:?}");
}

/// 모델에게 보여 주는 후보 목록 줄에는 옷장 id 가 들어간다 — 응답의 id 로 옷을 고른다
/// (`services::item_resolution`). 기본·자세한 형식 둘 다다.
#[test]
fn shortlist_lines_carry_request_refs_that_resolve_to_the_listed_item() {
    use style_engine::services::item_resolution::{Resolution, resolve_item};
    let none = HashSet::new();
    let w = wardrobe();
    let sl = build_all_shortlists(&w, &ctx(19.6, &none));
    let offered = sl.offered_items();
    for g in [sl.to_grouped(), sl.to_grouped_detailed()] {
        assert!(!g.tops.is_empty());
        for (lines, items, cat, p) in [
            (&g.tops, &sl.tops, "상의", "T"),
            (&g.bottoms, &sl.bottoms, "하의", "B"),
        ] {
            for (i, (line, c)) in lines.iter().zip(items).enumerate() {
                let r = format!("{p}{}", i + 1);
                assert!(line.contains(&format!("| id:{r} |")), "{line}");
                assert!(
                    !line.contains(&format!("id:{}", c.id)),
                    "실제 UUID 는 보이지 않는다: {line}"
                );
                match resolve_item(cat, Some(&r), &c.name, &offered) {
                    Resolution::Resolved { clothing, .. } => assert_eq!(clothing.id, c.id),
                    other => panic!("{r}: {other:?}"),
                }
                // 서버가 만든 변형 후보는 실제 id 로도 연결된다.
                assert!(matches!(
                    resolve_item(cat, Some(&c.id), "", &offered),
                    Resolution::Resolved { .. }
                ));
            }
        }
    }
}
