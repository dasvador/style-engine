//! 엣지 추운 날 같은 묶음의 외투 차이(`recommendation_service::prefer_distinct_outer`).
//!
//! 다른 조합이 오늘의 추천과 같은 차콜 롱 울 코트면, 점수가 가까운 후보 중 외투가 두 축 이상 다른
//! 것을 고른다. 품질이 크게 낮은 후보, 후보가 하나뿐인 경우, 오늘의 추천과 상하의가 같은 후보로는
//! 바꾸지 않는다. DB·LLM 을 쓰지 않는다.

use style_engine::models::clothing::Clothing;
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::{
    Role, Saturation, Silhouette, Style, StyleGenre, Thickness, Tone,
};
use style_engine::services::recommendation_service as svc;

fn item(
    id: &str,
    category: &str,
    color: &str,
    material: &str,
    sil: Silhouette,
    kw: &str,
) -> Clothing {
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
        silhouette_volume: Some(sil),
        material_primary: Some(material.into()),
        sub_category: None,
        floating_score: None,
        strong_style_score: None,
        texture_keywords: Some(kw.into()),
        created_at: now,
        updated_at: now,
    }
}

fn wardrobe() -> Vec<Clothing> {
    use Silhouette::*;
    vec![
        item("knit", "상의", "블랙", "wool", Regular, "rib knit"),
        item("tee", "상의", "블랙", "cotton", Regular, "jersey"),
        item("turtle", "상의", "그레이", "wool", Regular, "knit"),
        item("slacks", "하의", "블랙", "wool", Regular, "wide leg"),
        item("skirt", "하의", "블랙", "leather", Regular, "midi skirt"),
        item("denim", "하의", "블랙", "denim", Regular, "slim leg"),
        item(
            "charcoal",
            "아우터",
            "차콜",
            "wool",
            Regular,
            "mid-calf length, insulated lining",
        ),
        item(
            "black_coat",
            "아우터",
            "블랙",
            "wool",
            Regular,
            "mid-calf length, insulated lining",
        ),
        item(
            "puffer",
            "아우터",
            "페트롤틸",
            "nylon",
            Oversized,
            "cocoon, down fill, hip-length",
        ),
        item("boots", "신발", "블랙", "leather", Regular, "boots"),
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

fn run(cands: &[OutfitCandidate]) -> (Option<usize>, Option<usize>) {
    let w = wardrobe();
    let t = svc::select_todays_pick(cands, -5.0, &w);
    let v = t
        .as_ref()
        .and_then(|x| svc::select_variation(cands, &x.candidate));
    let v = svc::prefer_distinct_outer(cands, t.as_ref(), v, -5.0, &w, true);
    (
        t.map(|r| r.candidate.ai_candidate_index),
        v.map(|r| r.candidate.ai_candidate_index),
    )
}

/// 다른 조합이 같은 차콜 코트면 점수가 가까운 퍼퍼 후보로 바꾼다.
#[test]
fn same_outer_in_batch_is_swapped_for_a_close_distinct_one() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 80),
        cand(1, "tee", "skirt", "charcoal", 78),
        cand(2, "turtle", "denim", "puffer", 74),
    ];
    assert_eq!(run(&cands), (Some(0), Some(2)));
}

/// 블랙 롱 울 코트는 차콜 롱 울 코트와 한 축도 다르지 않다 — 다른 옷이어도 같아 보인다.
#[test]
fn a_lookalike_coat_does_not_count_as_different() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 80),
        cand(1, "tee", "skirt", "black_coat", 78),
        cand(2, "turtle", "denim", "puffer", 74),
    ];
    assert_eq!(run(&cands), (Some(0), Some(2)));
}

/// 품질이 크게 낮은(점수 차 10 초과) 후보로는 바꾸지 않는다. 카드도 없애지 않는다.
#[test]
fn far_lower_quality_is_not_promoted() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 80),
        cand(1, "tee", "skirt", "charcoal", 78),
        cand(2, "turtle", "denim", "puffer", 60),
    ];
    assert_eq!(run(&cands), (Some(0), Some(1)));
}

/// 오늘의 추천과 상하의가 같은 후보는 외투만 다르다고 다른 조합이 되지 않는다. 후보가 하나뿐이면
/// 카드 한 장 그대로다.
#[test]
fn same_top_bottom_or_single_candidate_keeps_the_result() {
    let cands = vec![
        cand(0, "knit", "slacks", "charcoal", 80),
        cand(1, "tee", "skirt", "charcoal", 78),
        cand(2, "knit", "slacks", "puffer", 79),
    ];
    let (t, v) = run(&cands);
    assert_eq!(t, Some(0));
    assert_ne!(v, Some(2));
    assert!(v.is_some());
    assert_eq!(run(&cands[..1]), (Some(0), None));
}
