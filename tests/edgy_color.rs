//! 엣지의 색 — 컬러 옷이 채점에서 근거 없이 밀리지 않는지, 두 카드가 무채색으로만 보일 때
//! 순위가 가까운 컬러 후보를 고르는지(`recommendation_service::prefer_visible_color`).
//!
//! 색은 이름이 아니라 저장된 색·채도로 본다. 시드 밖의 색(머스타드), 색을 모르는 옷, 무채색만
//! 있는 옷장, 가방·신발만 유색인 조합, 여민 코트 안의 컬러 상의, 품질이 낮은 컬러 후보를
//! 반례로 둔다. DB·LLM 을 쓰지 않는다.

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{IssueCode, OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::{Role, Saturation, Style, StyleGenre, Thickness, Tone};
use style_engine::services::mood_rules;
use style_engine::services::recommendation_service as svc;

fn item(id: &str, category: &str, color: &str, sat: Saturation, role: Role) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: id.into(),
        name: id.into(),
        category: category.into(),
        color: (!color.is_empty()).then(|| color.into()),
        thickness: Thickness::Medium,
        image_url: None,
        tone: Some(Tone::Dark),
        saturation: Some(sat),
        style: Some(Style::Basic),
        weight: None,
        role: Some(role),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(2),
        gender: Some("female".into()),
        style_mood: Some(StyleGenre::Edgy),
        visual_weight: None,
        texture_depth: None,
        visual_weight_v2: None,
        texture_depth_v2: None,
        grounding_score: None,
        shadow_tone: None,
        silhouette_volume: None,
        material_primary: Some("wool".into()),
        sub_category: None,
        floating_score: None,
        strong_style_score: None,
        texture_keywords: Some("structured shoulders".into()),
        created_at: now,
        updated_at: now,
    }
}

use Saturation::{High, Low, Mid};

fn wardrobe() -> Vec<Clothing> {
    vec![
        item("black_knit", "상의", "블랙", Low, Role::Base),
        item("white_tee", "상의", "화이트", Low, Role::Base),
        item("red_top", "상의", "체리레드", High, Role::SoftAccent),
        item("mustard_top", "상의", "머스타드", Mid, Role::SoftAccent), // 시드 밖의 색
        item("unknown_top", "상의", "", High, Role::SoftAccent),        // 색을 모르는 옷
        item("slacks", "하의", "블랙", Low, Role::Base),
        item("denim", "하의", "인디고", Low, Role::Base),
        item("charcoal_coat", "아우터", "차콜", Low, Role::Structural),
        item("plum_coat", "아우터", "플럼", Mid, Role::Accent),
        item("boots", "신발", "블랙", Low, Role::Structural),
        item("red_boots", "신발", "레드", High, Role::SoftAccent),
        item("black_bag", "가방", "블랙", Low, Role::Accent),
        item("wine_bag", "가방", "버건디", Mid, Role::Accent),
    ]
}

fn cand(
    idx: usize,
    top: &str,
    bottom: &str,
    outer: Option<&str>,
    shoes: &str,
    style: i32,
) -> OutfitCandidate {
    let mut c = OutfitCandidate::new(
        Some(top.into()),
        Some(bottom.into()),
        outer.map(Into::into),
        Some(shoes.into()),
        Some("black_bag".into()),
        idx,
    );
    c.style_score = style;
    c.final_score = style;
    c
}

fn pick(cands: &[OutfitCandidate], t: f64) -> (Option<usize>, Option<usize>) {
    let w = wardrobe();
    let todays = svc::select_todays_pick(cands, t, &w);
    let variation = todays
        .as_ref()
        .and_then(|x| svc::select_variation(cands, &x.candidate));
    let (a, b) = svc::prefer_visible_color(cands, todays, variation, t, &w);
    (
        a.map(|r| r.candidate.ai_candidate_index),
        b.map(|r| r.candidate.ai_candidate_index),
    )
}

/// 색은 저장된 색·채도로 본다. 시드 밖의 머스타드도 색이고, 색을 모르는 옷은 색이 아니다.
#[test]
fn chromatic_is_read_from_stored_colour() {
    let w = wardrobe();
    let get = |id: &str| w.iter().find(|c| c.id == id).unwrap();
    for id in ["red_top", "mustard_top", "plum_coat", "wine_bag"] {
        assert!(svc::is_chromatic(get(id)), "{id}");
    }
    for id in [
        "black_knit",
        "white_tee",
        "denim",
        "charcoal_coat",
        "unknown_top",
    ] {
        assert!(!svc::is_chromatic(get(id)), "{id}");
    }
    // 채도가 낮다고 적힌 색은 색으로 세지 않는다.
    let mut dull = item("dull", "상의", "올리브", Low, Role::Base);
    assert!(!svc::is_chromatic(&dull));
    dull.saturation = Some(Mid);
    assert!(svc::is_chromatic(&dull));
}

/// 보이는 색: 아우터의 색은 늘 보이고, 상의의 색은 아우터가 없거나 코트를 여미지 않는 기온에서만
/// 보인다. 신발·가방의 색은 세지 않는다.
#[test]
fn visible_colour_respects_closed_coats_and_ignores_accessories() {
    let w = wardrobe();
    let under_coat = cand(0, "red_top", "slacks", Some("charcoal_coat"), "boots", 80);
    assert!(svc::visible_color(&under_coat, &w, 2.0).is_none());
    assert!(svc::visible_color(&under_coat, &w, 16.0).is_some());
    let coat = cand(0, "black_knit", "slacks", Some("plum_coat"), "boots", 80);
    assert!(svc::visible_color(&coat, &w, 2.0).is_some());
    let mut shoes_only = cand(0, "black_knit", "slacks", None, "red_boots", 80);
    shoes_only.bag_id = Some("wine_bag".into());
    assert!(svc::visible_color(&shoes_only, &w, 16.0).is_none());
}

/// 무채색 오늘의 추천과 5점 안의 컬러 후보면 컬러가 오늘의 추천이다. 6점 넘게 낮거나, 날씨
/// 경계를 뺀 스타일이 60 미만이면 고르지 않는다.
#[test]
fn a_close_colour_candidate_takes_today_only_within_the_margin() {
    let near = [
        cand(0, "black_knit", "slacks", None, "boots", 84),
        cand(1, "red_top", "denim", None, "boots", 80),
    ];
    assert_eq!(pick(&near, 16.0).0, Some(1));
    let far = [
        cand(0, "black_knit", "slacks", None, "boots", 90),
        cand(1, "red_top", "denim", None, "boots", 80),
    ];
    assert_eq!(pick(&far, 16.0).0, Some(0));
    let mut weak = near.clone();
    weak[1].style_score = 64;
    weak[1].final_score = 82;
    weak[1].weather_penalty = 5;
    assert_eq!(pick(&weak, 16.0).0, Some(0));
}

/// 오늘의 추천과 다른 조합이 무채색으로만 보이면 다른 조합은 10점 안의 컬러 후보가 된다.
/// 2°C 에 여민 코트 안의 컬러 상의는 보이지 않으므로 컬러 아우터 후보가 대신 오른다.
#[test]
fn a_cold_variation_shows_colour_on_the_outerwear() {
    let cands = [
        cand(
            0,
            "black_knit",
            "slacks",
            Some("charcoal_coat"),
            "boots",
            90,
        ),
        cand(1, "red_top", "denim", Some("charcoal_coat"), "boots", 86),
        cand(2, "white_tee", "denim", Some("plum_coat"), "boots", 78),
    ];
    let (t, v) = pick(&cands, 2.0);
    assert_eq!(t, Some(0));
    assert_eq!(v, Some(2));
    // 16°C 에는 코트를 여미지 않으므로 레드 상의가 보인다. 4점 차이라 그 후보가 오늘의 추천이다.
    assert_eq!(pick(&cands, 16.0), (Some(1), Some(0)));
}

/// 무채색만 있는 옷장, 이미 색이 보이는 오늘의 추천에서는 아무것도 바꾸지 않는다.
#[test]
fn nothing_changes_without_a_colour_choice() {
    let plain = [
        cand(0, "black_knit", "slacks", None, "boots", 84),
        cand(1, "white_tee", "denim", None, "boots", 82),
    ];
    let w = wardrobe();
    let t = svc::select_todays_pick(&plain, 16.0, &w);
    let v = t
        .as_ref()
        .and_then(|x| svc::select_variation(&plain, &x.candidate));
    let before = (
        t.as_ref().map(|r| r.candidate.ai_candidate_index),
        v.as_ref().map(|r| r.candidate.ai_candidate_index),
    );
    assert_eq!(pick(&plain, 16.0), before);
    let coloured = [
        cand(0, "red_top", "slacks", None, "boots", 90),
        cand(1, "mustard_top", "denim", None, "boots", 88),
        cand(2, "white_tee", "denim", None, "boots", 89),
    ];
    assert_eq!(pick(&coloured, 16.0).0, Some(0));
}

fn ctx(items: &[&Clothing]) -> OutfitContext {
    OutfitContext {
        slots: items
            .iter()
            .map(|c| OutfitSlot {
                slot: match c.category.as_str() {
                    "상의" => SlotKind::Top,
                    "하의" => SlotKind::Bottom,
                    "아우터" => SlotKind::Outer,
                    "신발" => SlotKind::Shoes,
                    _ => SlotKind::Bag,
                },
                clothing: (*c).clone(),
                seasons: vec!["가을".into(), "겨울".into()],
                texture_worlds: Vec::new(),
            })
            .collect(),
        situation: Some("일상".into()),
    }
}

/// 컬러 코트(포인트)에 무채색 가방(포인트)이면 엣지는 "포인트 과다" 를 지운다. 유색 가방이거나
/// 옷 두 곳이 포인트면 그대로 깎는다. 가방 규칙은 남는다. 다른 무드는 그대로다.
#[test]
fn a_neutral_bag_does_not_compete_with_a_colour_coat() {
    let w = wardrobe();
    let get = |id: &str| w.iter().find(|c| c.id == id).unwrap();
    let too_many = |ids: &[&str], mood| {
        let items: Vec<&Clothing> = ids.iter().map(|i| get(i)).collect();
        mood_rules::evaluate_for_mood(&ctx(&items), Some("가을"), mood)
            .problems
            .iter()
            .any(|p| p.code == IssueCode::TooManyAccents)
    };
    let edgy = Some(StyleGenre::Edgy);
    let neutral_bag = ["white_tee", "denim", "plum_coat", "boots", "black_bag"];
    assert!(too_many(&neutral_bag, None));
    assert!(!too_many(&neutral_bag, edgy));
    assert!(too_many(
        &["white_tee", "denim", "plum_coat", "boots", "wine_bag"],
        edgy
    ));
    let mut accent_top = get("red_top").clone();
    accent_top.role = Some(Role::Accent);
    let two = [
        &accent_top,
        get("denim"),
        get("plum_coat"),
        get("boots"),
        get("black_bag"),
    ];
    let e = mood_rules::evaluate_for_mood(&ctx(&two), Some("가을"), edgy);
    assert!(
        e.problems
            .iter()
            .any(|p| p.code == IssueCode::TooManyAccents)
    );
    for g in [
        StyleGenre::Minimal,
        StyleGenre::ModernChic,
        StyleGenre::Street,
    ] {
        assert!(too_many(&neutral_bag, Some(g)), "{g}");
    }
}

/// Codex 검토 예시: 스타일 80·반복 −20 인 무채색(최종 60, 1단계)을 스타일 65 컬러(최종 65,
/// 3단계)로 바꾸지 않는다. 같은 단계의 컬러 후보면 예전처럼 5점 안에서 바꾼다.
#[test]
fn colour_never_drops_today_to_a_lower_tier() {
    let mut plain = cand(0, "black_knit", "slacks", None, "boots", 80);
    plain.recency_penalty = 20;
    plain.final_score = 60;
    let low = cand(1, "red_top", "denim", None, "boots", 65);
    assert_eq!(pick(&[plain.clone(), low], 16.0).0, Some(0));
    let mut same_tier = cand(1, "red_top", "denim", None, "boots", 72);
    same_tier.final_score = 58;
    assert_eq!(pick(&[plain, same_tier], 16.0).0, Some(1));
}

/// 오늘의 추천 적합도(2/3)를 못 채우는 컬러 후보는, 적합도를 채운 무채색 오늘의 추천을
/// 대신하지 못한다 — 최종 점수가 더 높아도 그렇다. 10°C 에 얇은 상의 단독은 기온 조건을,
/// 상하의가 같은 톤·같은 색온도면 대비 조건을 못 채운다.
#[test]
fn an_unqualified_colour_look_does_not_replace_a_qualified_one() {
    let mut w = wardrobe();
    let mut thin_red = item("thin_red", "상의", "체리레드", High, Role::SoftAccent);
    thin_red.thickness = Thickness::Thin;
    thin_red.color_temperature = Some("warm".into());
    let mut warm_slacks = item("warm_slacks", "하의", "블랙", Low, Role::Base);
    warm_slacks.color_temperature = Some("warm".into());
    w.push(thin_red);
    w.push(warm_slacks);
    let plain = cand(
        0,
        "black_knit",
        "slacks",
        Some("charcoal_coat"),
        "boots",
        80,
    );
    let mut red = cand(1, "thin_red", "warm_slacks", None, "boots", 90);
    red.final_score = 90;
    let todays = svc::select_todays_pick(&[plain.clone(), red.clone()], 10.0, &w);
    assert_eq!(
        todays.as_ref().map(|t| t.candidate.ai_candidate_index),
        Some(0)
    );
    let (t, _) = svc::prefer_visible_color(&[plain, red], todays, None, 10.0, &w);
    assert_eq!(t.map(|r| r.candidate.ai_candidate_index), Some(0));
}

/// 다른 조합도 기존 단계를 지킨다 — 스타일 68 이상인 무채색 다른 조합을 그보다 낮은 컬러
/// 후보로 바꾸지 않는다.
#[test]
fn colour_keeps_the_variation_style_bar() {
    let cands = [
        cand(
            0,
            "black_knit",
            "slacks",
            Some("charcoal_coat"),
            "boots",
            90,
        ),
        cand(1, "white_tee", "denim", Some("charcoal_coat"), "boots", 70),
        cand(2, "white_tee", "slacks", Some("plum_coat"), "boots", 66),
    ];
    // 66 은 다른 조합 점수(70)의 10점 안이고 컬러 하한(60)도 넘지만, 68 단계 아래다.
    assert_eq!(pick(&cands, 2.0).1, Some(1));
    // 68 단계를 넘는 컬러 후보면 바꾼다.
    let mut ok = cands.clone();
    ok[2].style_score = 68;
    ok[2].final_score = 68;
    assert_eq!(pick(&ok, 2.0).1, Some(2));
}

/// 색을 모르는 값·영문 무채색·금속색은 색이 아니다. 채도가 비어 있으면 이름이 분명한
/// 유채색일 때만 인정한다(시드 밖의 머스타드·아쿠아블루는 색이다). 채도를 지어내지 않는다.
#[test]
fn unknown_or_neutral_colour_strings_are_not_colour() {
    let with = |color: &str, sat: Option<Saturation>| {
        let mut c = item("x", "상의", color, Mid, Role::Base);
        c.saturation = sat;
        svc::is_chromatic(&c)
    };
    for color in [
        "미상",
        "unknown",
        "Unknown",
        "N/A",
        "-",
        "black",
        "Black",
        "off white",
        "실버",
        "샴페인",
        "카멜",
        "에스프레소브라운",
    ] {
        assert!(!with(color, Some(Mid)), "{color}");
        assert!(!with(color, None), "{color}");
    }
    for color in ["머스타드", "아쿠아블루", "코발트", "Cobalt Blue"] {
        assert!(with(color, None), "{color}");
    }
    // 이름만으로는 색인지 알 수 없는 값은 채도가 적혀 있을 때만 색이다.
    assert!(!with("로즈골드", None));
    assert!(with("로즈골드", Some(Mid)));
    assert!(!with("체리레드", Some(Low)));
}
