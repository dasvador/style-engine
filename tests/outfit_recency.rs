//! 최근 추천 이력에 따른 반복 감점.
//!
//! 채팅 경로는 점수 1등 조합을 그대로 돌려주므로, 옷장과 날씨가 그대로면 같은
//! 질문에 늘 같은 착장이 나왔다. 감점이 실제로 점수를 낮추는지 여기서 고정한다.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use style_engine::models::clothing::Clothing;
use style_engine::models::recommendation_history::OutfitRecommendationHistory;
use style_engine::models::style_vocab::{Role, Saturation, Style, Thickness, Tone, Weight};
use style_engine::services::outfit_scorer::{
    FeedbackContext, RecentHistory, total_outfit_score_full,
};

fn ts() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2026-10-07 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}

fn item(id: &str, name: &str, category: &str) -> Clothing {
    Clothing {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        color: Some("네이비".to_string()),
        thickness: Thickness::Medium,
        image_url: None,
        tone: Some(Tone::Mid),
        saturation: Some(Saturation::Low),
        style: Some(Style::Basic),
        weight: Some(Weight::Mid),
        role: Some(Role::Base),
        color_temperature: Some("neutral".to_string()),
        versatility: Some("flexible".to_string()),
        statement_level: Some(1),
        formality_level: Some(2),
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
        created_at: ts(),
        updated_at: ts(),
    }
}

fn history_row(top: &str, bottom: Option<&str>) -> OutfitRecommendationHistory {
    OutfitRecommendationHistory {
        id: "h".to_string(),
        user_id: "u".to_string(),
        top_id: Some(top.to_string()),
        bottom_id: bottom.map(|b| b.to_string()),
        outer_id: None,
        shoes_id: None,
        bag_id: None,
        recommended_at: ts(),
    }
}

fn no_feedback() -> FeedbackContext {
    FeedbackContext {
        item_adj: HashMap::new(),
        preference: HashMap::new(),
    }
}

#[test]
fn 이력에서_아이템별_등장_횟수를_센다() {
    let rows = vec![
        history_row("top-1", Some("bottom-1")),
        history_row("top-1", None),
        history_row("top-2", Some("bottom-1")),
    ];
    let recent = RecentHistory::from_history(&rows);

    assert_eq!(recent.item_freq.get("top-1"), Some(&2));
    assert_eq!(recent.item_freq.get("top-2"), Some(&1));
    assert_eq!(recent.item_freq.get("bottom-1"), Some(&2));
    assert_eq!(recent.item_freq.get("shoes-1"), None);
}

/// 이름이 아니라 id 로 센다 — 이력 표에 남는 것이 id 이고, 옷장에는 이름이 같은
/// 아이템이 둘 있을 수 있다.
#[test]
fn 같은_이름_다른_아이템은_따로_센다() {
    let rows = vec![history_row("top-1", None)];
    let recent = RecentHistory::from_history(&rows);

    let used = item("top-1", "네이비 니트", "상의");
    let twin = item("top-9", "네이비 니트", "상의");
    let bottom = item("bottom-1", "그레이 슬랙스", "하의");
    let shoes = item("shoes-1", "화이트 스니커", "신발");
    let fb = no_feedback();

    let used_score = total_outfit_score_full(&used, &[&used, &bottom, &shoes], None, &fb, &recent);
    let twin_score = total_outfit_score_full(&twin, &[&twin, &bottom, &shoes], None, &fb, &recent);

    assert!(
        twin_score > used_score,
        "이력에 없는 쪽이 높아야 한다 (twin={twin_score}, used={used_score})"
    );
}

#[test]
fn 최근에_쓴_아이템이_많을수록_더_깎인다() {
    let top = item("top-1", "네이비 니트", "상의");
    let bottom = item("bottom-1", "그레이 슬랙스", "하의");
    let shoes = item("shoes-1", "화이트 스니커", "신발");
    let outfit = [&top, &bottom, &shoes];
    let fb = no_feedback();

    let score = |rows: &[OutfitRecommendationHistory]| {
        total_outfit_score_full(&top, &outfit, None, &fb, &RecentHistory::from_history(rows))
    };

    let base = score(&[]);
    let once = score(&[history_row("top-1", None)]);
    let twice = score(&[history_row("top-1", None), history_row("top-1", None)]);
    let thrice = score(&[
        history_row("top-1", None),
        history_row("top-1", None),
        history_row("top-1", None),
    ]);

    assert_eq!(base - once, 2, "1회 사용은 -2");
    assert_eq!(base - twice, 5, "2회 사용은 -5");
    assert_eq!(base - thrice, 10, "3회 이상은 -10");
}

/// 착장 전체가 겹치면 아이템마다 깎여 합산된다 — 조합을 바꾸는 쪽이 유리해진다.
#[test]
fn 겹치는_아이템_수만큼_합산해_깎는다() {
    let top = item("top-1", "네이비 니트", "상의");
    let bottom = item("bottom-1", "그레이 슬랙스", "하의");
    let shoes = item("shoes-1", "화이트 스니커", "신발");
    let outfit = [&top, &bottom, &shoes];
    let fb = no_feedback();

    let base = total_outfit_score_full(&top, &outfit, None, &fb, &RecentHistory::from_history(&[]));
    let both = total_outfit_score_full(
        &top,
        &outfit,
        None,
        &fb,
        &RecentHistory::from_history(&[history_row("top-1", Some("bottom-1"))]),
    );

    assert_eq!(base - both, 4, "상의·하의 각각 -2");
}
