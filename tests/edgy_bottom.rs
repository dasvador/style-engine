//! 엣지 같은 묶음의 하의 차이(`recommendation_service::prefer_distinct_bottom`, 2026-11-18).
//!
//! 다른 조합의 하의가 오늘의 추천 하의와 두 축(색 묶음·종류·기장·소재) 미만으로만 다르면 — 차콜 울
//! 슬랙스 다음 블랙 울 바지 — 점수가 가까운 후보 중 하의가 두 축 이상 다른 것을 고른다. 품질이 크게
//! 낮은 후보, 오늘의 추천과 상하의가 같은 후보로는 바꾸지 않고, 추운 날 이미 맞춘 외투 차이를 잃지
//! 않는다. 데모 시드의 하의 축도 확인한다. DB·LLM 을 쓰지 않는다.

use style_engine::models::clothing::Clothing;
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::{
    Role, Saturation, Silhouette, Style, StyleGenre, Thickness, Tone,
};
use style_engine::services::look_signature::bottom_look;
use style_engine::services::recommendation_service as svc;

fn item(id: &str, category: &str, color: &str, material: &str, sub: &str, kw: &str) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: id.into(),
        name: id.into(),
        category: category.into(),
        color: Some(color.into()),
        thickness: Thickness::Thick,
        image_url: None,
        tone: Some(Tone::Dark),
        saturation: Some(Saturation::Mid),
        style: Some(Style::Basic),
        weight: None,
        role: Some(Role::Base),
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
        silhouette_volume: Some(Silhouette::Regular),
        material_primary: Some(material.into()),
        sub_category: Some(sub.into()),
        floating_score: None,
        strong_style_score: None,
        texture_keywords: Some(kw.into()),
        created_at: now,
        updated_at: now,
    }
}

fn wardrobe() -> Vec<Clothing> {
    vec![
        item("knit", "상의", "블랙", "wool", "니트", "rib knit"),
        item("tee", "상의", "블랙", "cotton", "긴팔탑", "jersey"),
        item("turtle", "상의", "그레이", "wool", "니트", "knit"),
        item("slacks", "하의", "차콜", "wool", "슬랙스", "wool, wide leg"),
        item(
            "barrel",
            "하의",
            "블랙",
            "wool",
            "와이드팬츠",
            "wool, barrel leg",
        ),
        item(
            "cord",
            "하의",
            "보틀그린",
            "corduroy",
            "와이드팬츠",
            "wide-wale corduroy, barrel leg, full length",
        ),
        item(
            "suede",
            "하의",
            "러스트",
            "suede",
            "스커트",
            "suede wrap, midi length",
        ),
        item(
            "charcoal",
            "아우터",
            "차콜",
            "wool",
            "코트",
            "mid-calf length, insulated lining",
        ),
        item(
            "puffer",
            "아우터",
            "페트롤틸",
            "nylon",
            "패딩",
            "cocoon, down fill, hip-length",
        ),
        item("boots", "신발", "블랙", "leather", "앵클부츠", "boots"),
    ]
}

fn cand(idx: usize, top: &str, bottom: &str, outer: &str, style: i32) -> OutfitCandidate {
    let mut c = OutfitCandidate::new(
        Some(top.into()),
        Some(bottom.into()),
        Some(outer.into()),
        Some("boots".into()),
        None,
        idx,
    );
    c.style_score = style;
    c.final_score = style;
    c
}

fn run_with(
    cands: &[OutfitCandidate],
    temp: f64,
    recent: &[String],
) -> (Option<usize>, Option<usize>) {
    let w = wardrobe();
    let t = svc::select_todays_pick(cands, temp, &w);
    let v = t
        .as_ref()
        .and_then(|x| svc::select_variation(cands, &x.candidate));
    let cold = temp < 12.0;
    let v = if cold {
        svc::prefer_distinct_outer(cands, t.as_ref(), v, temp, &w, true)
    } else {
        v
    };
    let v = svc::prefer_distinct_bottom(cands, t.as_ref(), v, temp, &w, true, cold, recent);
    (
        t.map(|r| r.candidate.ai_candidate_index),
        v.map(|r| r.candidate.ai_candidate_index),
    )
}

fn run(cands: &[OutfitCandidate], temp: f64) -> (Option<usize>, Option<usize>) {
    run_with(cands, temp, &[])
}

/// 차콜 울 와이드와 블랙 울 배럴은 같은 어두운 색·바지·롱·울 — 다른 옷이어도 같아 보인다. 점수가
/// 가까운 그린 코듀로이 바지로 바꾼다.
#[test]
fn lookalike_dark_trousers_are_swapped_for_a_close_distinct_bottom() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "barrel", "charcoal", 80),
        cand(2, "turtle", "cord", "charcoal", 76),
    ];
    assert_eq!(run(&cands, 15.0), (Some(0), Some(2)));
}

/// 이미 두 축 이상 다르면 그대로다.
#[test]
fn an_already_distinct_bottom_is_kept() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "suede", "charcoal", 80),
        cand(2, "turtle", "cord", "charcoal", 79),
    ];
    assert_eq!(run(&cands, 15.0), (Some(0), Some(1)));
}

/// 품질이 크게 낮은(점수 차 10 초과) 후보로는 바꾸지 않는다. 카드도 없애지 않는다. 후보가
/// 하나뿐이면 카드 한 장 그대로다.
#[test]
fn far_lower_quality_or_single_candidate_keeps_the_result() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "barrel", "charcoal", 80),
        cand(2, "turtle", "cord", "charcoal", 62),
    ];
    assert_eq!(run(&cands, 15.0), (Some(0), Some(1)));
    assert_eq!(run(&cands[..1], 15.0), (Some(0), None));
}

/// 추운 날: 외투 차이(퍼퍼)를 맞춘 다른 조합을, 외투가 오늘의 추천과 같은 후보로 되돌리지 않는다.
/// 외투도 다르고 하의도 다른 후보가 있으면 그것을 고른다.
#[test]
fn cold_day_keeps_the_outer_difference() {
    let only_same_outer = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "barrel", "puffer", 80),
        cand(2, "turtle", "cord", "charcoal", 79),
    ];
    assert_eq!(run(&only_same_outer, -5.0), (Some(0), Some(1)));
    let both = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "barrel", "puffer", 80),
        cand(2, "turtle", "cord", "charcoal", 79),
        cand(3, "tee", "cord", "puffer", 78),
    ];
    assert_eq!(run(&both, -5.0), (Some(0), Some(3)));
}

/// 다른 조합의 하의가 직전 요청 카드의 하의면, 이미 두 축 이상 달라도 점수가 가까운 다른 하의로
/// 바꾼다. 바꿀 후보가 없으면 그대로다.
#[test]
fn a_bottom_from_the_previous_request_is_swapped_when_a_close_one_exists() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 82),
        cand(1, "tee", "suede", "charcoal", 80),
        cand(2, "turtle", "cord", "charcoal", 78),
    ];
    let recent = vec!["suede".to_string()];
    assert_eq!(run_with(&cands, 15.0, &recent), (Some(0), Some(2)));
    assert_eq!(run_with(&cands[..2], 15.0, &recent), (Some(0), Some(1)));
}

/// 데모 시드(`seeds/demo/edgy_bottom_colour.sql`)의 축: 기존 어두운 바지와 색·소재가 다르다.
#[test]
fn seed_bottoms_read_as_new_colours_and_surfaces() {
    let leather = item(
        "버건디 레더 스트레이트 팬츠",
        "하의",
        "버건디",
        "leather",
        "팬츠",
        "matte burgundy lambskin leather, straight leg, full length to the ankle",
    );
    let cord = item(
        "보틀그린 코듀로이 배럴 팬츠",
        "하의",
        "보틀그린",
        "corduroy",
        "와이드팬츠",
        "wide-wale cotton corduroy, curved barrel leg, full length to the ankle",
    );
    let suede = item(
        "러스트 스웨이드 비대칭 랩 미디 스커트",
        "하의",
        "러스트",
        "suede",
        "스커트",
        "brushed rust suede, wrap front, asymmetric diagonal hem, midi length to mid-calf",
    );
    let l = |c: &Clothing| bottom_look(c).unwrap().label();
    assert_eq!(l(&leather), "레드·바지·롱·가죽");
    assert_eq!(l(&cord), "그린·바지·롱·코듀로이");
    assert_eq!(l(&suede), "오렌지·치마·미디·스웨이드");
    let velvet = item(
        "딥 플럼 벨벳 슬릿 미니스커트",
        "하의",
        "플럼",
        "velvet",
        "스커트",
        "deep plum velvet, clean A-line mini skirt, mid-thigh hem, small offset front slit",
    );
    assert_eq!(l(&velvet), "퍼플·치마·미니·벨벳");
    assert!(bottom_look(&velvet).unwrap().bares_legs());
    let charcoal = &wardrobe()[3];
    assert_eq!(l(charcoal), "어두운 무채색·바지·롱·울");
}

/// 시드와 롤백이 같은 id 네 개만 다룬다.
#[test]
fn seed_and_rollback_cover_the_same_ids() {
    let ids = |path: &str| {
        let sql = std::fs::read_to_string(path).unwrap();
        let mut v: Vec<String> = sql
            .match_indices("'0ffd1200-")
            .map(|(i, _)| sql[i + 1..i + 37].to_string())
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let seed = ids("seeds/demo/edgy_bottom_colour.sql");
    assert_eq!(seed, ids("seeds/demo/edgy_bottom_colour_rollback.sql"));
    assert_eq!(seed.len(), 4);
    for (id, end) in seed.iter().zip(["0301", "0302", "0303", "0304"]) {
        assert!(id.ends_with(end), "{id}");
    }
}

/// 첫 카드 외투가 직전 요청의 첫 카드 외투(퍼퍼)와 같으면 같은 단계·가까운 점수의 다른 외투 후보로
/// 바꾼다. 점수 차가 크면 그대로다. 직전 외투가 없으면 그대로다.
#[test]
fn first_card_outer_differs_from_the_previous_first_card() {
    use style_engine::services::look_signature::outer_look;
    let w = wardrobe();
    let puffer = outer_look(w.iter().find(|c| c.id == "puffer").unwrap()).unwrap();
    let pick = |cands: &[OutfitCandidate], last: Option<&_>| {
        let t = svc::select_todays_pick(cands, -5.0, &w);
        let (t, changed) = svc::prefer_fresh_today_outer(cands, t, last, -5.0, &w, true);
        (t.map(|r| r.candidate.ai_candidate_index), changed)
    };
    let close = vec![
        cand(0, "knit", "cord", "puffer", 84),
        cand(1, "tee", "slacks", "charcoal", 81),
    ];
    // 퍼퍼에 색이 보여 바꿀 후보도 색이 보여야 한다 — 차콜 코트 후보는 무채색이라 그대로다.
    assert_eq!(pick(&close, Some(&puffer)), (Some(0), false));
    let mut w2 = close.clone();
    w2.push(cand(2, "turtle", "suede", "puffer", 83));
    assert_eq!(pick(&w2, Some(&puffer)), (Some(0), false));
    // 색 조건이 없는(무채색 첫 카드) 경우: 직전과 같은 차콜 코트면 가까운 퍼퍼 후보로.
    let charcoal = outer_look(w.iter().find(|c| c.id == "charcoal").unwrap()).unwrap();
    let dark = vec![
        cand(0, "knit", "slacks", "charcoal", 84),
        cand(1, "tee", "barrel", "puffer", 82),
    ];
    assert_eq!(pick(&dark, Some(&charcoal)), (Some(1), true));
    let far = vec![
        cand(0, "knit", "slacks", "charcoal", 84),
        cand(1, "tee", "barrel", "puffer", 70),
    ];
    assert_eq!(pick(&far, Some(&charcoal)), (Some(0), false));
    assert_eq!(pick(&dark, None), (Some(0), false));
}
