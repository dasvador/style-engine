//! 엣지 추천의 선택 경로 — 채점(`mood_rules`) → 날씨 게이트(`serving_ranker`, 경계 −10)
//! → 무드 게이트(`mood_gate`) → 반복 보정(`finalize_scores`) → 카드 선택.
//!
//! 점수 몇 점을 맞추지 않는다. 어떤 후보가 뽑히고 어떤 후보가 왜 빠지는지를 본다. 옷은
//! `seeds/demo/edgy.sql` 과 같은 속성으로 만들되, 시드에 없는 옷(`holdout_*`)도 섞어 특정
//! 상품에만 맞춘 수정이 아닌지 확인한다. DB·LLM 을 쓰지 않는다.

use std::collections::HashSet;

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{IssueCode, OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::{
    Role, Saturation, Style, StyleGenre, Thickness, Tone, Weight,
};
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::{WEATHER_BORDERLINE_PENALTY, compute_today_fit};
use style_engine::services::shortlist::{ShortlistContext, build_all_shortlists};
use style_engine::services::style_engine as engine;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules, unfit_gate};

use Role::*;
use Thickness::*;

#[allow(clippy::too_many_arguments)]
fn item(
    name: &str,
    category: &str,
    tone: Tone,
    role: Role,
    formality: i8,
    thickness: Thickness,
    material: &str,
    sub: &str,
    keywords: &str,
) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: name.into(),
        name: name.into(),
        category: category.into(),
        color: Some(
            match tone {
                Tone::Dark => "블랙",
                Tone::Mid => "그레이",
                Tone::Bright => "화이트",
            }
            .into(),
        ),
        thickness,
        image_url: None,
        tone: Some(tone),
        saturation: Some(Saturation::Low),
        style: Some(Style::Basic),
        weight: Some(Weight::Mid),
        role: Some(role),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(formality),
        gender: Some("female".into()),
        style_mood: Some(StyleGenre::Edgy),
        visual_weight: None,
        texture_depth: None,
        visual_weight_v2: None,
        texture_depth_v2: None,
        grounding_score: None,
        shadow_tone: None,
        silhouette_volume: None,
        material_primary: Some(material.into()),
        sub_category: (!sub.is_empty()).then(|| sub.into()),
        floating_score: None,
        strong_style_score: None,
        texture_keywords: (!keywords.is_empty()).then(|| keywords.into()),
        created_at: now,
        updated_at: now,
    }
}

// ─── 옷 (시드와 같은 속성) ───
fn asym_jersey() -> Clothing {
    item(
        "차콜 비대칭 헴 저지 롱슬리브",
        "상의",
        Tone::Dark,
        SoftAccent,
        2,
        Thin,
        "jersey",
        "긴팔탑",
        "matte cotton jersey, asymmetric hem dipping longer on one side, long sleeves, close fit",
    )
}
fn structured_top() -> Clothing {
    item(
        "블랙 스트럭처드 숄더 하이넥 탑",
        "상의",
        Tone::Dark,
        Accent,
        3,
        Medium,
        "jersey",
        "긴팔탑",
        "firm ponte jersey, structured padded shoulders, high neck, long sleeves, fitted body",
    )
}
fn slim_knit() -> Clothing {
    item(
        "블랙 리브드 슬림 니트",
        "상의",
        Tone::Dark,
        Base,
        2,
        Medium,
        "knit",
        "니트",
        "ribbed knit, slim fit",
    )
}
fn tee() -> Clothing {
    item(
        "화이트 코튼 크루넥 티셔츠",
        "상의",
        Tone::Bright,
        Base,
        1,
        Thin,
        "cotton",
        "반팔티셔츠",
        "cotton jersey, relaxed crewneck",
    )
}
fn barrel() -> Clothing {
    item(
        "차콜 울 배럴 레그 팬츠",
        "하의",
        Tone::Dark,
        Structural,
        3,
        Medium,
        "wool",
        "와이드팬츠",
        "wool blend, curved barrel leg, front pleats, high waist, ankle length",
    )
}
fn leather_skirt() -> Clothing {
    item(
        "블랙 레더 미디 스커트",
        "하의",
        Tone::Dark,
        SoftAccent,
        3,
        Medium,
        "leather",
        "스커트",
        "smooth matte leather, straight cut, midi length, back slit",
    )
}
fn denim() -> Clothing {
    item(
        "인디고 와이드 데님",
        "하의",
        Tone::Mid,
        Base,
        2,
        Medium,
        "denim",
        "데님팬츠",
        "denim, wide leg, rigid",
    )
}
fn mini() -> Clothing {
    item(
        "올리브 비대칭 헴 랩 미니스커트",
        "하의",
        Tone::Mid,
        SoftAccent,
        2,
        Medium,
        "cotton",
        "스커트",
        "cotton twill, wrap front, asymmetric hem, mini length",
    )
}
fn boxy_jacket() -> Clothing {
    item(
        "블랙 박시 크롭 울 재킷",
        "아우터",
        Tone::Dark,
        Structural,
        3,
        Medium,
        "wool",
        "크롭자켓",
        "dense wool twill, boxy cropped cut, sculpted shoulders, concealed zip front",
    )
}
fn leather_blazer() -> Clothing {
    item(
        "브라운 레더 오버사이즈 블레이저",
        "아우터",
        Tone::Dark,
        Accent,
        3,
        Medium,
        "leather",
        "블레이저",
        "matte lambskin leather, structured shoulders, oversized single-breasted cut, hip-length",
    )
}
fn lined_coat() -> Clothing {
    item(
        "차콜 비대칭 여밈 롱 울 코트",
        "아우터",
        Tone::Dark,
        Structural,
        3,
        Thick,
        "wool",
        "롱코트",
        "heavy wool, structured shoulders, asymmetric overlapping front with off-centre buttons, mid-calf length, insulated lining",
    )
}
fn rider() -> Clothing {
    item(
        "블랙 레더 라이더 자켓",
        "아우터",
        Tone::Dark,
        Structural,
        3,
        Medium,
        "leather",
        "라이더자켓",
        "leather, cropped biker, silver hardware",
    )
}
fn combat_boots() -> Clothing {
    item(
        "블랙 레더 컴뱃 부츠",
        "신발",
        Tone::Dark,
        Structural,
        2,
        Medium,
        "leather",
        "앵클부츠",
        "smooth leather, lace-up front, chunky lug sole, ankle-high",
    )
}
fn knee_boots() -> Clothing {
    item(
        "블랙 레더 니하이 롱부츠",
        "신발",
        Tone::Dark,
        Structural,
        3,
        Medium,
        "leather",
        "롱부츠",
        "smooth leather, knee-high slim shaft, block heel",
    )
}
fn sandals() -> Clothing {
    item(
        "블랙 레더 청키 피셔맨 샌들",
        "신발",
        Tone::Dark,
        Connector,
        2,
        Medium,
        "leather",
        "샌들",
        "matte leather, woven fisherman straps, open sides, chunky lug sole",
    )
}
fn leather_bag() -> Clothing {
    item(
        "블랙 레더 지오메트릭 숄더백",
        "가방",
        Tone::Dark,
        SoftAccent,
        3,
        Medium,
        "leather",
        "숄더백",
        "matte leather, angular trapezoid shape, rigid body, short strap",
    )
}

// ─── 시드 밖의 옷: 업로드 옷처럼 설명이 비어 있다 ───
fn holdout_uneven_knit() -> Clothing {
    item(
        "네이비 언발란스 헴 니트",
        "상의",
        Tone::Dark,
        SoftAccent,
        2,
        Medium,
        "knit",
        "니트",
        "",
    )
}
fn holdout_plain_blazer() -> Clothing {
    item(
        "블랙 싱글 테일러드 자켓",
        "아우터",
        Tone::Dark,
        Structural,
        3,
        Medium,
        "polyester",
        "자켓",
        "structured, single breasted",
    )
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
                seasons: vec!["가을".into(), "겨울".into()],
                texture_worlds: Vec::new(),
            })
            .collect(),
        situation: Some("일상".into()),
    }
}

fn score(items: &[Clothing], mood: Option<StyleGenre>) -> i32 {
    mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), mood).score
}

/// 운영 경로처럼 후보를 만들어 고른다: 엣지 채점 → 날씨(실패는 빼고 경계는 −10) → 무드
/// 게이트(무드 후보가 없으면 그대로) → 오늘의 추천·다른 조합. 돌려주는 것은 인덱스.
fn select(outfits: &[Vec<Clothing>], temperature: f64) -> (Option<usize>, Option<usize>) {
    let wardrobe: Vec<Clothing> = outfits.iter().flatten().cloned().collect();
    let mut cands: Vec<OutfitCandidate> = outfits
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let id_of = |k: SlotKind| o.iter().find(|c| kind(c) == k).map(|c| c.id.clone());
            let mut c = OutfitCandidate::new(
                id_of(SlotKind::Top),
                id_of(SlotKind::Bottom),
                id_of(SlotKind::Outer),
                id_of(SlotKind::Shoes),
                id_of(SlotKind::Bag),
                i,
            );
            c.style_score = score(o, Some(StyleGenre::Edgy));
            c
        })
        .collect();
    svc::finalize_scores(&mut cands);
    let fit =
        |c: &OutfitCandidate| compute_today_fit(&ctx(&outfits[c.ai_candidate_index]), temperature);
    let (mut servable, _) =
        unfit_gate::keep_wearable_strict(cands, |c| fit(c) == TodayFitLevel::Fail);
    for c in &mut servable {
        if fit(c) == TodayFitLevel::Borderline {
            c.weather_penalty = WEATHER_BORDERLINE_PENALTY;
            c.final_score -= WEATHER_BORDERLINE_PENALTY;
        }
    }
    servable.sort_by_key(|c| std::cmp::Reverse(c.final_score));
    let off_mood = |c: &OutfitCandidate| {
        let items: Vec<&Clothing> = outfits[c.ai_candidate_index].iter().collect();
        mood_gate::reads_as(StyleGenre::Edgy, &items) == Some(false)
    };
    let (servable, _) = unfit_gate::keep_wearable(servable, off_mood);
    let todays = svc::select_todays_pick(&servable, temperature, &wardrobe);
    let variation = todays
        .as_ref()
        .and_then(|t| svc::select_variation(&servable, &t.candidate));
    let idx =
        |r: &Option<svc::ModeSelectionResult>| r.as_ref().map(|r| r.candidate.ai_candidate_index);
    (idx(&todays), idx(&variation))
}

/// 기본 티·데님에 가죽 부츠·가방만 더한 조합은 점수가 높아도 엣지 자리에 오르지 않는다.
/// 비대칭 상의 + 배럴 팬츠, 시드 밖의 언발란스 니트 + 데님이 대신 오른다.
#[test]
fn leather_accessories_on_basics_do_not_take_the_edgy_cards() {
    let basic = vec![tee(), denim(), combat_boots(), leather_bag()];
    let shaped = vec![asym_jersey(), barrel(), combat_boots(), leather_bag()];
    let holdout = vec![
        holdout_uneven_knit(),
        denim(),
        combat_boots(),
        leather_bag(),
    ];
    assert!(score(&basic, Some(StyleGenre::Edgy)) >= score(&shaped, Some(StyleGenre::Edgy)) - 10);
    let (t, v) = select(&[basic, shaped, holdout], 16.0);
    assert!(t.is_some_and(|i| i != 0), "{t:?}");
    assert!(v.is_none_or(|i| i != 0), "{v:?}");
    let picked: HashSet<usize> = [t, v].into_iter().flatten().collect();
    assert!(picked.contains(&1) || picked.contains(&2));
}

/// 엣지 채점은 근거가 맞지 않는 감점만 되돌린다: 조용한 아우터 안의 형태 있는 상의,
/// 형태가 중심인 하의의 슬롯 역할, 가죽과 울이 섞인 어두운 코디. 가산점은 없다 — 무드 없는
/// 평가보다 낮아지는 일이 없고, 되돌린 감점 말고는 같다.
#[test]
fn edgy_waives_only_the_misplaced_deductions() {
    let look = vec![
        structured_top(),
        leather_skirt(),
        boxy_jacket(),
        combat_boots(),
        leather_bag(),
    ];
    let plain = engine::evaluate(&ctx(&look), Some("가을"));
    let edgy = mood_rules::evaluate_for_mood(&ctx(&look), Some("가을"), Some(StyleGenre::Edgy));
    let codes = |e: &style_engine::models::outfit::EvaluationResult| {
        e.problems
            .iter()
            .map(|p| p.code.clone())
            .collect::<Vec<_>>()
    };
    assert!(codes(&plain).contains(&IssueCode::LackOfContrast));
    assert!(codes(&plain).contains(&IssueCode::StrongInner));
    assert!(!codes(&edgy).contains(&IssueCode::LackOfContrast));
    assert!(!codes(&edgy).contains(&IssueCode::StrongInner));
    assert!(edgy.score > plain.score);
    // 격식 상한(홈 '일상' 2.5)은 엣지에서 넓히지 않는다.
    assert_eq!(
        codes(&plain).contains(&IssueCode::FormalitySituationMismatch),
        codes(&edgy).contains(&IssueCode::FormalitySituationMismatch)
    );

    // 하의가 형태의 중심이면 슬롯 역할 감점을 지운다. 상의도 포인트면 그대로 둔다.
    let bottom_center = vec![slim_knit(), leather_skirt(), combat_boots()];
    let both = vec![structured_top(), leather_skirt(), combat_boots()];
    let skirt_role = |items: &[Clothing]| {
        mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), Some(StyleGenre::Edgy))
            .problems
            .iter()
            .any(|p| p.code == IssueCode::SlotRoleMismatch && p.detail.contains("레더 미디 스커트"))
    };
    assert!(!skirt_role(&bottom_center));
    assert!(skirt_role(&both));

    // 형태도 가죽도 없는 어두운 코디는 그대로 깎는다.
    let dull = vec![
        slim_knit(),
        item(
            "블랙 와이드 슬랙스",
            "하의",
            Tone::Dark,
            Base,
            2,
            Medium,
            "polyester",
            "슬랙스",
            "wide leg, clean drape",
        ),
        combat_boots(),
    ];
    assert_eq!(score(&dull, Some(StyleGenre::Edgy)), score(&dull, None));

    // 다른 무드는 무드 없는 평가 그대로다.
    for g in [
        StyleGenre::ModernChic,
        StyleGenre::Street,
        StyleGenre::Minimal,
        StyleGenre::Mannish,
    ] {
        assert_eq!(score(&look, Some(g)), plain.score, "{g}");
    }
}

/// 2°C: 안감 없는 라이더·레더 블레이저는 겨울 외투가 아니다(경계, 얇은 탑이면 실패).
/// 보온 안감이 적힌 롱코트에 긴 하의면 통과다. 미니에 앵클부츠는 코트가 길어도 실패다.
#[test]
fn winter_edgy_needs_real_insulation_and_covered_legs() {
    let fit = |o: &[Clothing], t| compute_today_fit(&ctx(o), t);
    assert_eq!(
        fit(&[slim_knit(), barrel(), rider(), combat_boots()], 2.0),
        TodayFitLevel::Borderline
    );
    assert_eq!(
        fit(&[asym_jersey(), barrel(), rider(), combat_boots()], 2.0),
        TodayFitLevel::Fail
    );
    assert_eq!(
        fit(
            &[slim_knit(), barrel(), leather_blazer(), combat_boots()],
            2.0
        ),
        TodayFitLevel::Borderline
    );
    assert_eq!(
        fit(&[slim_knit(), barrel(), lined_coat(), combat_boots()], 2.0),
        TodayFitLevel::Pass
    );
    assert_eq!(
        fit(
            &[slim_knit(), leather_skirt(), lined_coat(), combat_boots()],
            2.0
        ),
        TodayFitLevel::Pass
    );
    assert_eq!(
        fit(&[slim_knit(), mini(), lined_coat(), combat_boots()], 2.0),
        TodayFitLevel::Fail
    );
    assert_eq!(
        fit(&[slim_knit(), mini(), lined_coat(), knee_boots()], 2.0),
        TodayFitLevel::Borderline
    );
    // 8°C 에는 라이더도 니트 위면 통과, 미니는 니하이 부츠가 아니면 경계다.
    assert_eq!(
        fit(&[slim_knit(), barrel(), rider(), combat_boots()], 8.0),
        TodayFitLevel::Pass
    );
    assert_eq!(
        fit(&[slim_knit(), mini(), boxy_jacket(), combat_boots()], 8.0),
        TodayFitLevel::Borderline
    );
    // 시각적 무게는 보온이 아니다 — 무거워 보이는 레더 블레이저도 같은 판정이다.
    let mut heavy = leather_blazer();
    heavy.weight = Some(Weight::Heavy);
    assert_eq!(
        fit(&[slim_knit(), barrel(), heavy], 2.0),
        fit(&[slim_knit(), barrel(), leather_blazer()], 2.0)
    );

    // 선택: 아우터만 다른 두 조합이면 2°C 의 오늘의 추천은 안감 롱코트 조합이다 — 라이더
    // 조합은 경계(−10)로 밀린다.
    let rider_look = vec![slim_knit(), denim(), rider(), combat_boots(), leather_bag()];
    let coat_look = vec![
        slim_knit(),
        denim(),
        lined_coat(),
        combat_boots(),
        leather_bag(),
    ];
    assert!(
        score(&rider_look, Some(StyleGenre::Edgy)) + 10 > score(&coat_look, Some(StyleGenre::Edgy))
    );
    let mini_look = vec![
        asym_jersey(),
        mini(),
        lined_coat(),
        combat_boots(),
        leather_bag(),
    ];
    let (t, _) = select(&[rider_look.clone(), coat_look, mini_look.clone()], 2.0);
    assert_eq!(t, Some(1));
    // 맞는 후보가 없으면 실패한 후보를 대신 내지 않는다.
    let (t, v) = select(
        &[
            mini_look,
            vec![asym_jersey(), denim(), rider(), combat_boots()],
        ],
        2.0,
    );
    assert_eq!((t, v), (None, None));
}

/// 26°C: 후보 목록에 코트·아우터·가죽 하의·울 하의가 없다. 형태 있는 상의와 미니·데님,
/// 샌들로 엣지가 남는다. 2°C 에는 샌들이 빠진다.
#[test]
fn hot_and_cold_pools_follow_the_weather() {
    let wardrobe = vec![
        asym_jersey(),
        structured_top(),
        slim_knit(),
        tee(),
        holdout_uneven_knit(),
        barrel(),
        leather_skirt(),
        denim(),
        mini(),
        boxy_jacket(),
        leather_blazer(),
        lined_coat(),
        rider(),
        holdout_plain_blazer(),
        combat_boots(),
        knee_boots(),
        sandals(),
        leather_bag(),
    ];
    let pool = |t: f64, season: &str| {
        let none = HashSet::new();
        let sl = build_all_shortlists(
            &wardrobe,
            &ShortlistContext {
                temperature: t,
                situation: Some("일상"),
                current_season: Some(season),
                recent_item_ids: &none,
            },
        );
        [sl.tops, sl.bottoms, sl.outers, sl.shoes, sl.bags]
            .concat()
            .into_iter()
            .map(|c| c.name.clone())
            .collect::<HashSet<String>>()
    };
    let hot = pool(26.0, "여름");
    for gone in [
        "블랙 레더 미디 스커트",
        "차콜 울 배럴 레그 팬츠",
        "차콜 비대칭 여밈 롱 울 코트",
        "블랙 레더 라이더 자켓",
        "브라운 레더 오버사이즈 블레이저",
    ] {
        assert!(!hot.contains(gone), "{gone}");
    }
    for kept in [
        "차콜 비대칭 헴 저지 롱슬리브",
        "올리브 비대칭 헴 랩 미니스커트",
        "블랙 레더 청키 피셔맨 샌들",
    ] {
        assert!(hot.contains(kept), "{kept}");
    }
    let cold = pool(2.0, "겨울");
    assert!(!cold.contains("블랙 레더 청키 피셔맨 샌들"));
    assert!(cold.contains("차콜 비대칭 여밈 롱 울 코트"));
    // 26°C 의 엣지 코디는 날씨 판정을 통과한다.
    assert_eq!(
        compute_today_fit(
            &ctx(&[asym_jersey(), mini(), sandals(), leather_bag()]),
            26.0
        ),
        TodayFitLevel::Pass
    );
}

/// 2°C 실측(2026-10-17)의 재현: 경계인 라이더 조합(스타일 79, 경계 −10, 반복 −27)과 통과인
/// 안감 롱코트 조합(스타일 65). '스타일 70점 이상' 조건은 경계 감점을 뺀 점수로 보므로
/// 라이더 조합이 혼자 1차를 통과하지 못하고, 최종 점수가 높은 코트 조합이 오늘의 추천이다.
/// 날씨 경계가 없으면 예전처럼 스타일 70점 이상인 후보가 먼저다 — 바뀐 것은 날씨 몫뿐이다.
#[test]
fn a_borderline_look_does_not_clear_the_style_bar_on_style_alone() {
    let rider_look = [slim_knit(), denim(), rider(), combat_boots(), leather_bag()];
    let coat_look = [
        asym_jersey(),
        barrel(),
        lined_coat(),
        combat_boots(),
        leather_bag(),
    ];
    let wardrobe: Vec<Clothing> = rider_look.iter().chain(coat_look.iter()).cloned().collect();
    let make = |i: usize, o: &[Clothing], style: i32, recency: i32, weather: i32| {
        let id_of = |k: SlotKind| o.iter().find(|c| kind(c) == k).map(|c| c.id.clone());
        let mut c = OutfitCandidate::new(
            id_of(SlotKind::Top),
            id_of(SlotKind::Bottom),
            id_of(SlotKind::Outer),
            id_of(SlotKind::Shoes),
            id_of(SlotKind::Bag),
            i,
        );
        c.style_score = style;
        c.recency_penalty = recency;
        c.weather_penalty = weather;
        c.final_score = style - recency - weather;
        c
    };
    let pick = |cands: &[OutfitCandidate]| {
        svc::select_todays_pick(cands, 2.0, &wardrobe).map(|r| r.candidate.ai_candidate_index)
    };
    let borderline_rider = make(0, &rider_look, 79, 27, WEATHER_BORDERLINE_PENALTY);
    let coat = make(1, &coat_look, 65, 0, 0);
    assert_eq!(pick(&[borderline_rider, coat.clone()]), Some(1));
    let passing = make(0, &rider_look, 79, 27, 0);
    assert_eq!(pick(&[passing, coat]), Some(0));
}

/// 엣지 올블랙 보정의 근거: 형태 디테일 또는 확인된 소재 대비. 소재를 모르는 옷은 근거가
/// 아니다(가죽 + 미상은 대비가 아니다). 가죽끼리여도 형태가 있으면 근거다.
#[test]
fn dark_edgy_contrast_needs_shape_or_a_confirmed_material_contrast() {
    let contrast = |items: &[Clothing]| {
        let refs: Vec<&Clothing> = items.iter().collect();
        mood_rules::dark_edgy_look_has_contrast(&refs)
    };
    let mut unknown_top = item(
        "블랙 기본 탑",
        "상의",
        Tone::Dark,
        Base,
        2,
        Thin,
        "",
        "",
        "",
    );
    unknown_top.material_primary = None;
    // 가죽 + 소재 미상: 대비가 아니다.
    assert!(!contrast(&[unknown_top.clone(), leather_skirt()]));
    // 가죽 + 확인된 니트: 대비다.
    assert!(contrast(&[slim_knit(), leather_skirt()]));
    // 가죽끼리, 형태 없음(기본 라이더 + 가죽 스커트 + 미상 탑): 근거가 없다.
    assert!(!contrast(&[unknown_top.clone(), leather_skirt(), rider()]));
    // 가죽끼리여도 비대칭 지퍼 같은 형태가 있으면 근거다.
    let mut zip_jacket = rider();
    zip_jacket.texture_keywords = Some("smooth leather, asymmetric zip front, cropped".into());
    assert!(contrast(&[
        unknown_top.clone(),
        leather_skirt(),
        zip_jacket.clone()
    ]));
    // 운영 점수에도 같은 근거가 쓰인다: 형태가 있는 가죽 코디는 올블랙 감점이 없고, 근거 없는
    // 코디는 남는다.
    let dark_codes = |items: &[Clothing]| {
        mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), Some(StyleGenre::Edgy))
            .problems
            .iter()
            .any(|p| p.code == IssueCode::LackOfContrast)
    };
    let boots = combat_boots();
    assert!(dark_codes(&[
        unknown_top.clone(),
        leather_skirt(),
        rider(),
        boots.clone()
    ]));
    assert!(!dark_codes(&[
        unknown_top,
        leather_skirt(),
        zip_jacket,
        boots
    ]));
}
