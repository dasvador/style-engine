//! 팝 추천의 선택 경로 — 채점(`mood_rules`) → 날씨 게이트(`serving_ranker`, 경계 −10)
//! → 무드 게이트(`mood_gate::reads_as_at`) → 반복 보정(`finalize_scores`) → 카드 선택.
//!
//! 점수 몇 점을 맞추지 않는다. 어떤 후보가 뽑히고 어떤 후보가 왜 빠지는지를 본다. 옷은
//! `seeds/demo/pop.sql` 과 같은 속성으로 만들되, 시드에 없는 업로드 옷(설명 없는 이름뿐인 옷)도
//! 섞는다. 판정은 id·이름 규칙·팝 태그가 아니라 저장된 색·채도·설명으로 한다. DB·LLM 을 쓰지
//! 않는다.

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{IssueCode, OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::style_vocab::{Role, Saturation, Style, StyleGenre, Thickness, Weight};
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::{WEATHER_BORDERLINE_PENALTY, compute_today_fit};
use style_engine::services::style_engine as engine;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules, unfit_gate};

use Role::*;
use Saturation::*;
use Thickness::*;

#[allow(clippy::too_many_arguments)]
fn item(
    name: &str,
    category: &str,
    color: &str,
    saturation: Saturation,
    role: Role,
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
        color: Some(color.into()),
        thickness,
        image_url: None,
        tone: None,
        saturation: Some(saturation),
        style: Some(Style::Basic),
        weight: Some(Weight::Mid),
        role: Some(role),
        color_temperature: None,
        versatility: None,
        statement_level: None,
        formality_level: Some(2),
        gender: Some("female".into()),
        style_mood: None,
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

fn cobalt_knit() -> Clothing {
    item(
        "코발트 블루 크롭 니트",
        "상의",
        "코발트",
        High,
        SoftAccent,
        Medium,
        "knit",
        "니트",
        "soft cotton-blend knit, cropped length, crew neck, long sleeves",
    )
}

fn block_sweat() -> Clothing {
    item(
        "레드 핑크 컬러블록 스웨트셔츠",
        "상의",
        "레드/핑크",
        High,
        Accent,
        Medium,
        "cotton",
        "맨투맨",
        "brushed cotton fleece, colour-block panels in red and pink, ribbed hem",
    )
}

fn cherry_skirt() -> Clothing {
    item(
        "체리레드 A라인 미디 스커트",
        "하의",
        "체리레드",
        High,
        SoftAccent,
        Medium,
        "cotton",
        "스커트",
        "cotton twill, a-line, midi length",
    )
}

fn green_cords() -> Clothing {
    item(
        "켈리그린 코듀로이 스트레이트 팬츠",
        "하의",
        "켈리그린",
        High,
        SoftAccent,
        Medium,
        "corduroy",
        "팬츠",
        "fine-wale corduroy, straight leg, full length",
    )
}

fn red_trucker() -> Clothing {
    item(
        "레드 크롭 트러커 재킷",
        "아우터",
        "레드",
        High,
        SoftAccent,
        Medium,
        "cotton",
        "트러커자켓",
        "cotton twill, cropped trucker cut, chest pockets",
    )
}

fn block_windbreaker() -> Clothing {
    item(
        "코발트 옐로 컬러블록 바람막이",
        "아우터",
        "코발트/옐로",
        High,
        Accent,
        Thin,
        "nylon",
        "윈드브레이커",
        "lightweight nylon, colour-block panels in cobalt blue and yellow, zip front, unlined",
    )
}

fn check_coat() -> Clothing {
    let mut c = item(
        "그린 레드 체크 울 코트",
        "아우터",
        "그린/레드",
        High,
        Accent,
        Thick,
        "wool",
        "코트",
        "heavy wool, bold check in green and red, knee-length, insulated lining",
    );
    c.weight = Some(Weight::Heavy);
    c
}

fn white_tee() -> Clothing {
    item(
        "화이트 코튼 크루넥 티셔츠",
        "상의",
        "화이트",
        Low,
        Base,
        Thin,
        "cotton",
        "반팔티셔츠",
        "cotton jersey, relaxed crewneck",
    )
}

fn black_knit() -> Clothing {
    item(
        "블랙 리브드 슬림 니트",
        "상의",
        "블랙",
        Low,
        Base,
        Medium,
        "knit",
        "니트",
        "ribbed knit, slim fit",
    )
}

fn denim() -> Clothing {
    item(
        "인디고 와이드 데님",
        "하의",
        "인디고",
        Low,
        Base,
        Medium,
        "denim",
        "데님팬츠",
        "rigid denim, wide leg, full length",
    )
}

fn chinos() -> Clothing {
    item(
        "베이지 치노 팬츠",
        "하의",
        "베이지",
        Low,
        Base,
        Medium,
        "cotton",
        "팬츠",
        "cotton twill, straight leg, full length",
    )
}

fn navy_coat() -> Clothing {
    let mut c = item(
        "네이비 울 코트",
        "아우터",
        "네이비",
        Low,
        Structural,
        Thick,
        "wool",
        "코트",
        "heavy wool, knee-length, insulated lining",
    );
    c.weight = Some(Weight::Heavy);
    c
}

fn sneakers() -> Clothing {
    item(
        "화이트 레더 로우 스니커즈",
        "신발",
        "화이트",
        Low,
        Base,
        Medium,
        "leather",
        "스니커즈",
        "",
    )
}

fn ankle_boots() -> Clothing {
    item(
        "블랙 레더 앵클부츠",
        "신발",
        "블랙",
        Low,
        Base,
        Medium,
        "leather",
        "앵클부츠",
        "",
    )
}

/// 무채색 포인트 가방(데이터상 역할 '포인트').
fn black_bag() -> Clothing {
    item(
        "블랙 레더 미니 숄더백",
        "가방",
        "블랙",
        Low,
        Accent,
        Medium,
        "leather",
        "숄더백",
        "",
    )
}

/// 색 있는 그래픽 가방 — 옷이 아니라 팝의 근거가 아니다.
fn graphic_bag() -> Clothing {
    item(
        "멀티컬러 그래픽 토트백",
        "가방",
        "레드/블루",
        High,
        Accent,
        Medium,
        "canvas",
        "토트백",
        "graphic print in red and blue",
    )
}

// ─── 경로 ───

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

fn reads(items: &[Clothing], temperature: f64) -> bool {
    let refs: Vec<&Clothing> = items.iter().collect();
    mood_gate::reads_as_at(StyleGenre::Pop, &refs, Some(temperature)) == Some(true)
}

/// 운영 경로처럼 후보를 만들어 고른다: 팝 채점 → 날씨(실패는 빼고 경계는 −10) → 무드
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
            c.style_score = score(o, Some(StyleGenre::Pop));
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
    let off_mood = |c: &OutfitCandidate| !reads(&outfits[c.ai_candidate_index], temperature);
    let (servable, _) = unfit_gate::keep_wearable(servable, off_mood);
    let todays = svc::select_todays_pick(&servable, temperature, &wardrobe);
    let variation = todays
        .as_ref()
        .and_then(|t| svc::select_variation(&servable, &t.candidate));
    let idx =
        |r: &Option<svc::ModeSelectionResult>| r.as_ref().map(|r| r.candidate.ai_candidate_index);
    (idx(&todays), idx(&variation))
}

fn codes(items: &[Clothing], mood: Option<StyleGenre>) -> Vec<(IssueCode, String)> {
    mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), mood)
        .problems
        .into_iter()
        .map(|p| (p.code, p.detail))
        .collect()
}

fn has(codes: &[(IssueCode, String)], code: IssueCode, about: &str) -> bool {
    codes.iter().any(|(c, d)| *c == code && d.contains(about))
}

// ─── 테스트 ───

/// 세 방향 모두 팝이다 — 서로 다른 색 계열 두 벌, 색 있는 배색 옷 한 벌, 배색 아우터. 선명한
/// 옷 한 벌에 기본색, 차분한 색, 흑백 무늬, 가방만 색인 조합은 아니다. 시드 밖 업로드 옷
/// ("레드 체크 스커트", 설명 없음)도 이름과 색으로 읽힌다.
#[test]
fn pop_reads_three_directions_and_not_their_lookalikes() {
    assert!(reads(&[cobalt_knit(), cherry_skirt(), sneakers()], 19.6));
    assert!(reads(&[block_sweat(), denim(), sneakers()], 19.6));
    assert!(reads(
        &[white_tee(), denim(), block_windbreaker(), sneakers()],
        19.6
    ));
    let upload = item(
        "레드 체크 스커트",
        "하의",
        "레드",
        High,
        SoftAccent,
        Medium,
        "",
        "",
        "",
    );
    assert!(reads(&[white_tee(), upload, sneakers()], 19.6));

    let red_tee = item(
        "레드 크루넥 티셔츠",
        "상의",
        "레드",
        High,
        SoftAccent,
        Thin,
        "cotton",
        "반팔티셔츠",
        "cotton jersey",
    );
    let navy_shirt = item(
        "네이비 옥스퍼드 셔츠",
        "상의",
        "네이비",
        Low,
        Base,
        Thin,
        "cotton",
        "셔츠",
        "oxford cotton",
    );
    let muted_top = item(
        "더스티 세이지 니트",
        "상의",
        "세이지",
        Low,
        Base,
        Medium,
        "knit",
        "니트",
        "soft knit",
    );
    let muted_bottom = item(
        "더스티 로즈 스커트",
        "하의",
        "로즈핑크",
        Low,
        Base,
        Medium,
        "cotton",
        "스커트",
        "",
    );
    let mono_check = item(
        "블랙 화이트 체커보드 니트",
        "상의",
        "블랙/화이트",
        Low,
        Accent,
        Medium,
        "knit",
        "니트",
        "checkerboard knit in black and white",
    );
    let lined_trench = item(
        "베이지 트렌치",
        "아우터",
        "베이지",
        Low,
        Structural,
        Medium,
        "cotton",
        "트렌치코트",
        "cotton twill, plain striped lining",
    );
    for (name, look) in [
        ("레드 티 + 데님", vec![red_tee, denim(), sneakers()]),
        ("네이비 셔츠 + 치노", vec![navy_shirt, chinos(), sneakers()]),
        ("차분한 색", vec![muted_top, muted_bottom, sneakers()]),
        ("흑백 무늬", vec![mono_check, denim(), sneakers()]),
        (
            "안감 줄무늬",
            vec![white_tee(), denim(), lined_trench, sneakers()],
        ),
        (
            "가방만 그래픽",
            vec![white_tee(), denim(), sneakers(), graphic_bag()],
        ),
    ] {
        assert!(!reads(&look, 19.6), "{name}");
    }
}

/// 12°C 아래에서는 코트를 여민다고 보므로 코트 속 상의의 색·무늬는 근거가 아니다. 기본색
/// 코트 속 배색 스웨트셔츠는 팝이 아니고, 배색 코트이거나 색 하의가 보이면 팝이다. 같은 착장도
/// 12°C 이상이면 상의가 보인다.
#[test]
fn a_closed_coat_hides_the_top_but_not_the_coat_or_the_bottom() {
    let hidden = vec![block_sweat(), denim(), navy_coat(), ankle_boots()];
    assert!(!reads(&hidden, 8.0));
    assert!(reads(&hidden, 14.0));
    assert!(reads(
        &[black_knit(), denim(), check_coat(), ankle_boots()],
        8.0
    ));
    // 코트 속 코발트 니트는 안 보이고, 체리레드 스커트 하나만 보인다 — 선명한 옷 한 벌.
    assert!(!reads(
        &[cobalt_knit(), cherry_skirt(), navy_coat(), ankle_boots()],
        8.0
    ));
    // 보이는 색이 둘(그린 하의 + 레드 재킷)이면 팝이다.
    assert!(reads(
        &[black_knit(), green_cords(), red_trucker(), ankle_boots()],
        8.0
    ));
}

/// 팝 채점은 근거가 맞지 않는 감점만 되돌린다: 색 하의·상의의 슬롯 역할, 무채색 포인트 가방이
/// 더한 포인트 과다. 옷 두 곳이 포인트인 조합, 베이스 없이 색 옷만 셋인 조합, 가방의 슬롯
/// 역할·가방 규칙은 그대로 깎는다. 가산점은 없다.
#[test]
fn pop_waives_only_the_misplaced_deductions() {
    // 두 색 조합: 하의의 슬롯 역할 감점이 사라진다. 가방의 슬롯 역할은 남는다.
    let two = vec![cobalt_knit(), cherry_skirt(), sneakers(), black_bag()];
    let plain = codes(&two, None);
    let pop = codes(&two, Some(StyleGenre::Pop));
    assert!(has(&plain, IssueCode::SlotRoleMismatch, "체리레드"));
    assert!(!has(&pop, IssueCode::SlotRoleMismatch, "체리레드"));
    assert!(has(&pop, IssueCode::SlotRoleMismatch, "숄더백"));
    assert!(score(&two, Some(StyleGenre::Pop)) > score(&two, None));

    // 배색 옷 한 벌 + 무채색 포인트 가방: 포인트 과다를 지운다.
    let one = vec![block_sweat(), denim(), sneakers(), black_bag()];
    assert!(has(&codes(&one, None), IssueCode::TooManyAccents, ""));
    assert!(!has(
        &codes(&one, Some(StyleGenre::Pop)),
        IssueCode::TooManyAccents,
        ""
    ));
    // 색 있는 포인트 가방이면 그대로 깎는다.
    let loud_bag = vec![block_sweat(), denim(), sneakers(), graphic_bag()];
    assert_eq!(
        has(&codes(&loud_bag, None), IssueCode::TooManyAccents, ""),
        has(
            &codes(&loud_bag, Some(StyleGenre::Pop)),
            IssueCode::TooManyAccents,
            ""
        )
    );

    // 옷 두 곳이 포인트(배색 스웨트 + 배색 바람막이)면 포인트 과다를 그대로 깎는다.
    let two_accents = vec![block_sweat(), denim(), block_windbreaker(), sneakers()];
    assert!(has(
        &codes(&two_accents, Some(StyleGenre::Pop)),
        IssueCode::TooManyAccents,
        ""
    ));
    // 베이스 없는 색 옷 셋은 중심축 부재를 그대로 깎는다.
    let three = vec![cobalt_knit(), green_cords(), red_trucker(), ankle_boots()];
    assert_eq!(
        has(&codes(&three, None), IssueCode::LackOfStructure, ""),
        has(
            &codes(&three, Some(StyleGenre::Pop)),
            IssueCode::LackOfStructure,
            ""
        )
    );
    // 기본색만인 조합은 팝 채점에서도 같은 점수다.
    let basic = vec![white_tee(), denim(), sneakers(), black_bag()];
    assert_eq!(score(&basic, Some(StyleGenre::Pop)), score(&basic, None));
    // 다른 무드는 무드 없는 평가 그대로다.
    let plain_two = engine::evaluate(&ctx(&two), Some("가을")).score;
    for g in [
        StyleGenre::Street,
        StyleGenre::Romantic,
        StyleGenre::Minimal,
    ] {
        assert_eq!(score(&two, Some(g)), plain_two, "{g}");
    }
}

/// 19.6°C: 점수가 같거나 높아도 레드 티 + 데님, 기본 티 + 데님 + 그래픽 가방은 팝 카드에
/// 오르지 않는다. 두 색 조합과 배색 옷 조합이 오늘의 추천·다른 조합에 오른다.
#[test]
fn lookalikes_do_not_take_the_pop_cards() {
    let red_tee = item(
        "레드 크루넥 티셔츠",
        "상의",
        "레드",
        High,
        SoftAccent,
        Thin,
        "cotton",
        "반팔티셔츠",
        "cotton jersey",
    );
    let outfits = vec![
        vec![red_tee, denim(), sneakers()],
        vec![white_tee(), denim(), sneakers(), graphic_bag()],
        vec![cobalt_knit(), cherry_skirt(), sneakers(), black_bag()],
        vec![block_sweat(), denim(), sneakers(), black_bag()],
    ];
    let (t, v) = select(&outfits, 19.6);
    for i in [t, v] {
        assert!(i.is_some_and(|i| i >= 2), "{t:?} {v:?}");
    }
    assert_ne!(t, v);
}

/// 2°C: 안감 없는 배색 바람막이는 겨울 외투가 아니라 고르지 않는다. 보온 안감이 적힌 체크 울
/// 코트에 긴 하의가 오른다. 코트 속 배색 스웨트는 근거가 아니어서, 기본색 코트 조합은 팝 카드가
/// 되지 못한다.
#[test]
fn winter_pop_needs_real_insulation_and_visible_colour() {
    let windbreaker = vec![block_sweat(), denim(), block_windbreaker(), ankle_boots()];
    let navy = vec![block_sweat(), denim(), navy_coat(), ankle_boots()];
    let check = vec![black_knit(), denim(), check_coat(), ankle_boots()];
    assert_ne!(
        compute_today_fit(&ctx(&windbreaker), 2.0),
        TodayFitLevel::Pass
    );
    assert_eq!(compute_today_fit(&ctx(&check), 2.0), TodayFitLevel::Pass);
    let (t, _) = select(&[windbreaker, navy, check], 2.0);
    assert_eq!(t, Some(2));
}
