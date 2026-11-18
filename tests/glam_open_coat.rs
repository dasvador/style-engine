//! 홈 글램 — 코트를 열었을 때 보이는 착장의 글램 근거(2026-11-19 사용자 신고).
//!
//! 실제 카드(3.1°C, 홈 '일상', 배치 6082cb02): 오늘의 추천이 블랙 아워글라스 롱 울 코트 + 블랙
//! 리브드 슬림 니트 + 화이트 와이드 데님 + 블랙 레더 앵클부츠 + 블랙 미니 체인 숄더백이었다. 코트
//! 설명의 "hourglass" 하나로 착장 전체가 글램 게이트를 지났고, 코트를 연 룩북 사진은 미니멀이었다.
//! 같은 코트 + 샴페인 메탈릭 니트 + 블랙 새틴 스커트 + 실버 메탈릭 미니백은 다른 조합 자리에 있었다.
//!
//! 옷 속성은 개발 DB 의 실제 행 그대로다. DB·LLM 을 쓰지 않는다.

use std::collections::HashSet;

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::compute_today_fit;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules, unfit_gate};

const T: f64 = 3.1;

/// DB 행: (id, 이름, 분류, 색, 두께, 톤, 채도, 스타일, 무게, 역할, 격식, 실루엣, 소재, 세부, 설명).
type Row<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    i8,
    Option<&'a str>,
    &'a str,
    &'a str,
    &'a str,
);

fn item(r: Row) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: r.0.into(),
        name: r.1.into(),
        category: r.2.into(),
        color: Some(r.3.into()),
        thickness: r.4.parse().unwrap(),
        image_url: None,
        tone: Some(r.5.parse().unwrap()),
        saturation: Some(r.6.parse().unwrap()),
        style: Some(r.7.parse().unwrap()),
        weight: Some(r.8.parse().unwrap()),
        role: Some(r.9.parse().unwrap()),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(r.10),
        gender: Some("female".into()),
        style_mood: Some(StyleGenre::Glam),
        visual_weight: None,
        texture_depth: None,
        visual_weight_v2: None,
        texture_depth_v2: None,
        grounding_score: None,
        shadow_tone: None,
        silhouette_volume: r.11.map(|s| s.parse().unwrap()),
        material_primary: Some(r.12.into()),
        sub_category: Some(r.13.into()),
        floating_score: None,
        strong_style_score: None,
        texture_keywords: Some(r.14.into()),
        created_at: now,
        updated_at: now,
    }
}

fn hourglass_coat() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-00000000000f",
        "블랙 아워글라스 롱 울 코트",
        "아우터",
        "블랙",
        "thick",
        "어두움",
        "낮음",
        "포멀",
        "중간",
        "구조템",
        4,
        Some("regular"),
        "wool",
        "롱코트",
        "heavy wool blend, hourglass silhouette, defined shoulders, nipped waist, flared hem, mid-calf length, wide peaked lapels, matte buttons, insulated lining",
    ))
}
fn fur_half_coat() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-00000000000e",
        "에스프레소 브라운 페이크퍼 하프코트",
        "아우터",
        "에스프레소브라운",
        "thick",
        "어두움",
        "중간",
        "베이직",
        "무거움",
        "포인트",
        3,
        Some("relaxed"),
        "faux fur",
        "하프코트",
        "plush faux fur, hip-length half coat, full but not rounded, wide collar, concealed closure, insulated lining",
    ))
}
fn ribbed_knit() -> Clothing {
    item((
        "d8929e84-546b-11f1-af43-264d116ef584",
        "블랙 리브드 슬림 니트",
        "상의",
        "블랙",
        "medium",
        "어두움",
        "낮음",
        "베이직",
        "중간",
        "베이스",
        2,
        Some("slim"),
        "knit",
        "니트",
        "ribbed knit, slim fit",
    ))
}
fn metallic_knit() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-000000000003",
        "샴페인 메탈릭 니트 탑",
        "상의",
        "샴페인",
        "medium",
        "밝음",
        "낮음",
        "베이직",
        "중간",
        "포인트",
        3,
        Some("regular"),
        "knit",
        "니트",
        "fine lurex knit, subtle metallic sheen, crew neck, long sleeves",
    ))
}
fn one_shoulder() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-000000000001",
        "블랙 원숄더 저지 탑",
        "상의",
        "블랙",
        "thin",
        "어두움",
        "낮음",
        "베이직",
        "가벼움",
        "약한포인트",
        3,
        Some("slim"),
        "jersey",
        "원숄더탑",
        "matte stretch jersey, one-shoulder neckline, fitted",
    ))
}
fn lace_wrap_top() -> Clothing {
    item((
        "0ffd1000-0000-0000-0000-000000000002",
        "차콜그레이 비대칭 랩넥 레이스 트리밍 셔링 탑",
        "상의",
        "차콜그레이",
        "thin",
        "어두움",
        "낮음",
        "베이직",
        "가벼움",
        "포인트",
        3,
        Some("slim"),
        "jersey",
        "긴팔탑",
        "soft matte stretch jersey, asymmetric wrap neckline edged with fine grey lace, lace-trimmed hem, side ruching, long sleeves, fitted",
    ))
}
fn ruched_tube_top() -> Clothing {
    item((
        "0ffd1000-0000-0000-0000-000000000004",
        "퍼플 새틴 레이스 바스트 그레이 셔링 튜브탑",
        "상의",
        "그레이/퍼플",
        "thin",
        "중간",
        "낮음",
        "베이직",
        "가벼움",
        "포인트",
        3,
        Some("slim"),
        "jersey",
        "튜브탑",
        "one-piece strapless top: ruched grey stretch mesh over an opaque lining, attached purple satin bust band edged with dark purple lace, fitted",
    ))
}
fn white_denim() -> Clothing {
    item((
        "8c004cbc-598c-11f1-af43-264d116ef584",
        "화이트 와이드 데님 팬츠",
        "하의",
        "화이트",
        "medium",
        "밝음",
        "낮음",
        "베이직",
        "중간",
        "베이스",
        2,
        Some("oversized"),
        "denim",
        "와이드팬츠",
        "white denim, wide leg",
    ))
}
fn satin_skirt() -> Clothing {
    item((
        "8c004e42-598c-11f1-af43-264d116ef584",
        "블랙 미디 새틴 롱스커트",
        "하의",
        "블랙",
        "thin",
        "어두움",
        "낮음",
        "베이직",
        "가벼움",
        "베이스",
        3,
        Some("regular"),
        "satin",
        "롱스커트",
        "satin, midi length",
    ))
}
fn wide_slacks() -> Clothing {
    item((
        "d892aeb0-546b-11f1-af43-264d116ef584",
        "블랙 와이드 슬랙스",
        "하의",
        "블랙",
        "medium",
        "어두움",
        "낮음",
        "베이직",
        "중간",
        "베이스",
        4,
        Some("oversized"),
        "polyester",
        "슬랙스",
        "wide leg, clean drape",
    ))
}
fn ankle_boots() -> Clothing {
    item((
        "0ffd0700-0000-0000-0000-00000000000c",
        "블랙 레더 앵클부츠",
        "신발",
        "블랙",
        "medium",
        "어두움",
        "낮음",
        "베이직",
        "중간",
        "연결템",
        3,
        None,
        "leather",
        "앵클부츠",
        "leather, block heel ankle boot",
    ))
}
fn satin_pumps() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-00000000000b",
        "블랙 새틴 포인티드 펌프스",
        "신발",
        "블랙",
        "medium",
        "어두움",
        "낮음",
        "포멀",
        "중간",
        "베이스",
        4,
        None,
        "satin",
        "힐",
        "satin upper, pointed toe, mid heel",
    ))
}
fn chain_bag() -> Clothing {
    item((
        "d892c832-546b-11f1-af43-264d116ef584",
        "블랙 미니 체인 숄더백",
        "가방",
        "블랙",
        "medium",
        "어두움",
        "낮음",
        "베이직",
        "중간",
        "포인트",
        3,
        None,
        "leather",
        "숄더백",
        "leather, chain strap, mini",
    ))
}
fn silver_bag() -> Clothing {
    item((
        "0ffd0b00-0000-0000-0000-00000000000c",
        "실버 메탈릭 미니 숄더백",
        "가방",
        "실버",
        "medium",
        "밝음",
        "낮음",
        "베이직",
        "중간",
        "포인트",
        3,
        None,
        "leather",
        "숄더백",
        "metallic leather, mini, short shoulder strap",
    ))
}

/// 실제 오늘의 추천(미니멀).
fn reported_minimal() -> Vec<Clothing> {
    vec![
        ribbed_knit(),
        white_denim(),
        hourglass_coat(),
        ankle_boots(),
        chain_bag(),
    ]
}
/// 실제 다른 조합(글램).
fn reported_glam() -> Vec<Clothing> {
    vec![
        metallic_knit(),
        satin_skirt(),
        hourglass_coat(),
        satin_pumps(),
        silver_bag(),
    ]
}

fn home(o: &[Clothing]) -> Option<bool> {
    let refs: Vec<&Clothing> = o.iter().collect();
    mood_gate::reads_as_seen(StyleGenre::Glam, &refs, Some(T))
}

/// 신고된 두 착장을 그대로 대조한다. 예전 판정은 둘 다 글램이었다(코트만으로 통과).
#[test]
fn reported_cards_open_coat() {
    let minimal = reported_minimal();
    let glam = reported_glam();
    let old = |o: &[Clothing]| {
        let r: Vec<&Clothing> = o.iter().collect();
        mood_gate::reads_as_at(StyleGenre::Glam, &r, Some(T))
    };
    assert_eq!(old(&minimal), Some(true), "예전 판정이 통과시킨 착장");
    assert_eq!(home(&minimal), Some(false));
    assert_eq!(home(&glam), Some(true));
}

/// 코트 + 기본 상의·하의는 가방·신발이 무엇이든(평범한 체인 가방, 기본 부츠, 실버 미니백, 새틴
/// 펌프스) 글램이 아니다. 안의 상의나 하의 하나에 근거가 있으면 흑백만이어도 글램이다.
#[test]
fn inner_needs_its_own_evidence_black_and_white_allowed() {
    for (shoes, bag) in [
        (ankle_boots(), chain_bag()),
        (satin_pumps(), silver_bag()),
        (ankle_boots(), silver_bag()),
    ] {
        let basic = vec![
            ribbed_knit(),
            white_denim(),
            hourglass_coat(),
            shoes.clone(),
            bag.clone(),
        ];
        assert_eq!(home(&basic), Some(false), "{} {}", shoes.name, bag.name);
        let fur = vec![
            ribbed_knit(),
            wide_slacks(),
            fur_half_coat(),
            shoes.clone(),
            bag.clone(),
        ];
        assert_eq!(home(&fur), Some(false), "퍼 코트 + 기본 {}", bag.name);
    }
    // 흑백: 블랙 리브 니트 + 블랙 새틴 스커트, 블랙 원숄더 + 블랙 와이드 슬랙스, 화이트 데님 + 원숄더.
    for o in [
        vec![
            ribbed_knit(),
            satin_skirt(),
            hourglass_coat(),
            ankle_boots(),
            chain_bag(),
        ],
        vec![
            one_shoulder(),
            wide_slacks(),
            hourglass_coat(),
            ankle_boots(),
            chain_bag(),
        ],
        vec![
            one_shoulder(),
            white_denim(),
            hourglass_coat(),
            ankle_boots(),
            chain_bag(),
        ],
    ] {
        assert_eq!(home(&o), Some(true), "{}", o[0].name);
    }
}

/// 승인된 퍼 + 무광 트리밍 상의 짝(글램 규칙 6)은 그대로 글램이다. 같은 상의라도 아워글라스 울
/// 코트 아래에서는 상의 자체의 근거(비대칭 랩넥)가 있어야 한다 — 셔링 튜브탑은 아니다.
#[test]
fn approved_fur_and_trim_pair_is_kept() {
    let fur_tube = vec![
        ruched_tube_top(),
        wide_slacks(),
        fur_half_coat(),
        ankle_boots(),
        chain_bag(),
    ];
    assert_eq!(home(&fur_tube), Some(true));
    let wool_tube = vec![
        ruched_tube_top(),
        wide_slacks(),
        hourglass_coat(),
        ankle_boots(),
        chain_bag(),
    ];
    assert_eq!(home(&wool_tube), Some(false));
    let wool_wrap = vec![
        lace_wrap_top(),
        wide_slacks(),
        hourglass_coat(),
        ankle_boots(),
        chain_bag(),
    ];
    assert_eq!(home(&wool_wrap), Some(true));
}

/// 아우터 없는 착장과 다른 무드는 예전 판정과 같다.
#[test]
fn other_moods_and_outerless_outfits_unchanged() {
    let pool = [
        reported_minimal(),
        reported_glam(),
        vec![ribbed_knit(), satin_skirt(), ankle_boots(), chain_bag()],
        vec![ribbed_knit(), white_denim(), ankle_boots(), silver_bag()],
    ];
    for o in &pool {
        let r: Vec<&Clothing> = o.iter().collect();
        for g in StyleGenre::ALL.iter().filter(|g| **g != StyleGenre::Glam) {
            assert_eq!(
                mood_gate::reads_as_seen(*g, &r, Some(T)),
                mood_gate::reads_as_at(*g, &r, Some(T)),
                "{g}"
            );
        }
        if !o.iter().any(|c| c.category == "아우터") {
            assert_eq!(
                mood_gate::reads_as_seen(StyleGenre::Glam, &r, Some(T)),
                mood_gate::reads_as_at(StyleGenre::Glam, &r, Some(T))
            );
        }
    }
}

fn kind(c: &Clothing) -> SlotKind {
    match c.category.as_str() {
        "상의" => SlotKind::Top,
        "하의" => SlotKind::Bottom,
        "아우터" => SlotKind::Outer,
        "신발" => SlotKind::Shoes,
        _ => SlotKind::Bag,
    }
}

fn ctx(items: &[Clothing]) -> OutfitContext {
    OutfitContext {
        slots: items
            .iter()
            .map(|c| OutfitSlot {
                slot: kind(c),
                clothing: c.clone(),
                seasons: vec!["겨울".into()],
                texture_worlds: Vec::new(),
            })
            .collect(),
        situation: Some("일상".into()),
    }
}

/// 홈 경로와 같은 순서(채점 → 반복 보정 → 날씨 게이트 → 홈 무드 게이트 → 세 자리)로 고른다.
/// 미니멀 후보가 점수 1위여도 세 카드(오늘의 추천·다른 조합·안 입은 옷) 어디에도 오르지 않는다.
#[test]
fn minimal_never_reaches_any_card_when_glam_candidates_exist() {
    let outfits = [
        reported_minimal(),
        reported_glam(),
        vec![
            one_shoulder(),
            wide_slacks(),
            hourglass_coat(),
            ankle_boots(),
            chain_bag(),
        ],
        vec![
            ribbed_knit(),
            wide_slacks(),
            fur_half_coat(),
            ankle_boots(),
            chain_bag(),
        ],
    ];
    let wardrobe: Vec<Clothing> = outfits.iter().flatten().cloned().collect();
    let mut cands: Vec<OutfitCandidate> = outfits
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let id = |k: SlotKind| o.iter().find(|c| kind(c) == k).map(|c| c.id.clone());
            let mut c = OutfitCandidate::new(
                id(SlotKind::Top),
                id(SlotKind::Bottom),
                id(SlotKind::Outer),
                id(SlotKind::Shoes),
                id(SlotKind::Bag),
                i,
            );
            c.style_score =
                mood_rules::evaluate_for_mood(&ctx(o), Some("겨울"), Some(StyleGenre::Glam)).score;
            // 미니멀 후보를 가장 높게 둔다(실제 카드에서도 1위였다).
            if i == 0 {
                c.style_score += 30;
            }
            c
        })
        .collect();
    svc::finalize_scores(&mut cands);
    cands.sort_by_key(|c| std::cmp::Reverse(c.final_score));
    assert_eq!(cands[0].ai_candidate_index, 0);
    let (servable, _) = unfit_gate::keep_wearable_strict(cands, |c| {
        compute_today_fit(&ctx(&outfits[c.ai_candidate_index]), T) == TodayFitLevel::Fail
    });
    let (servable, dropped) = unfit_gate::keep_wearable(servable, |c| {
        home(&outfits[c.ai_candidate_index]) == Some(false)
    });
    assert_eq!(dropped, 2, "미니멀 + 퍼 코트 기본 착장");
    let todays = svc::select_todays_pick(&servable, T, &wardrobe);
    let variation = todays
        .as_ref()
        .and_then(|t| svc::select_variation(&servable, &t.candidate));
    let dormant_ids: HashSet<String> = wardrobe.iter().map(|c| c.id.clone()).collect();
    let dormant = todays.as_ref().and_then(|t| {
        let mut chosen = vec![&t.candidate];
        if let Some(v) = &variation {
            chosen.push(&v.candidate);
        }
        svc::select_dormant(&servable, &dormant_ids, &chosen)
    });
    let shown: Vec<usize> = [&todays, &variation, &dormant]
        .iter()
        .filter_map(|r| r.as_ref().map(|r| r.candidate.ai_candidate_index))
        .collect();
    assert!(!shown.is_empty());
    for i in &shown {
        assert_eq!(home(&outfits[*i]), Some(true), "카드 {i}");
    }
}

/// 글램 후보가 하나도 없으면 게이트는 비우지 않고 그대로 둔다(빈 추천보다 낫다). 그때 카드는
/// 무드 미달(`off_mood`)로 표시돼 글램으로 보이지 않는다 — 여기서는 판정만 본다.
#[test]
fn fallback_keeps_cards_but_they_are_not_glam() {
    let outfits = [
        reported_minimal(),
        vec![
            ribbed_knit(),
            wide_slacks(),
            fur_half_coat(),
            ankle_boots(),
            chain_bag(),
        ],
    ];
    let cands: Vec<OutfitCandidate> = (0..outfits.len())
        .map(|i| OutfitCandidate::new(None, None, None, None, None, i))
        .collect();
    let (kept, dropped) = unfit_gate::keep_wearable(cands, |c| {
        home(&outfits[c.ai_candidate_index]) == Some(false)
    });
    assert_eq!((kept.len(), dropped), (2, 0));
    assert!(outfits.iter().all(|o| home(o) == Some(false)));
}
