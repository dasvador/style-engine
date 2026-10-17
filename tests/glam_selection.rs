//! 글램 추천의 선택 경로 — 채점(`mood_rules`) → 무드 게이트(`mood_gate`) → 날씨 게이트
//! (`unfit_gate`) → 반복 보정(`finalize_scores`) → 카드 선택(`recommendation_service`).
//!
//! 점수 몇 점을 맞추지 않는다. 어떤 후보가 뽑히고 어떤 후보가 왜 빠지는지, 후보 사이의
//! 순서를 본다. 옷은 글램 시드와 같은 속성으로 만들되, 시드에 없는 옷(`holdout_*`)도 섞어
//! 특정 상품에만 맞춘 수정인지 확인한다. DB·LLM 을 쓰지 않는다.

use std::collections::HashSet;

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{IssueCode, OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::recommendation_history::OutfitHistorySummary;
use style_engine::models::style_vocab::{
    Role, Saturation, Style, StyleGenre, Thickness, Tone, Weight,
};
use style_engine::services::recommendation_diversity::calculate_recency_penalty;
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::compute_today_fit;
use style_engine::services::shortlist::{ShoeWeather, shoe_weather};
use style_engine::services::style_engine as engine;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules, unfit_gate};

#[allow(clippy::too_many_arguments)]
fn item(
    id: &str,
    category: &str,
    tone: Tone,
    role: Role,
    formality: i8,
    thickness: Thickness,
    material: &str,
    keywords: &str,
) -> Clothing {
    let now = chrono::NaiveDateTime::default();
    Clothing {
        id: id.into(),
        name: id.into(),
        category: category.into(),
        color: Some(
            if tone == Tone::Dark {
                "블랙"
            } else {
                "화이트"
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
        style_mood: Some(StyleGenre::Glam),
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

use Role::*;
use Thickness::*;
use Tone::*;

fn emerald_top() -> Clothing {
    item(
        "에메랄드 새틴 드레이프 블라우스",
        "상의",
        Mid,
        Accent,
        3,
        Thin,
        "satin",
        "fluid satin, draped cowl neckline",
    )
}
fn tee() -> Clothing {
    item(
        "화이트 코튼 크루넥 티셔츠",
        "상의",
        Bright,
        Base,
        1,
        Thin,
        "cotton",
        "cotton jersey, relaxed crewneck",
    )
}
fn black_tee() -> Clothing {
    item(
        "블랙 코튼 티셔츠",
        "상의",
        Dark,
        Base,
        1,
        Thin,
        "cotton",
        "cotton jersey",
    )
}
fn one_shoulder() -> Clothing {
    item(
        "블랙 원숄더 저지 탑",
        "상의",
        Dark,
        SoftAccent,
        3,
        Thin,
        "jersey",
        "matte stretch jersey, one-shoulder neckline",
    )
}
fn slacks() -> Clothing {
    item(
        "블랙 와이드 슬랙스",
        "하의",
        Dark,
        Base,
        4,
        Medium,
        "polyester",
        "wide leg, clean drape",
    )
}
fn denim() -> Clothing {
    item(
        "워시드 다크블루 와이드 데님",
        "하의",
        Dark,
        Base,
        2,
        Medium,
        "denim",
        "washed denim, dark, wide leg",
    )
}
fn satin_skirt() -> Clothing {
    item(
        "샴페인 새틴 슬립 미디스커트",
        "하의",
        Bright,
        Base,
        3,
        Thin,
        "satin",
        "bias-cut satin, slip skirt",
    )
}
fn black_satin_skirt() -> Clothing {
    item(
        "블랙 미디 새틴 롱스커트",
        "하의",
        Dark,
        Base,
        3,
        Thin,
        "satin",
        "satin, midi length",
    )
}
fn tailored_jacket() -> Clothing {
    item(
        "블랙 싱글 테일러드 자켓",
        "아우터",
        Dark,
        Structural,
        4,
        Medium,
        "polyester",
        "structured, single breasted",
    )
}
fn velvet_blazer() -> Clothing {
    item(
        "블랙 벨벳 테일러드 블레이저",
        "아우터",
        Dark,
        Accent,
        4,
        Medium,
        "velvet",
        "cotton velvet, soft pile sheen",
    )
}
fn ankle_boots() -> Clothing {
    item(
        "블랙 레더 앵클부츠",
        "신발",
        Dark,
        Connector,
        3,
        Medium,
        "leather",
        "leather, block heel ankle boot",
    )
}
fn clutch() -> Clothing {
    item(
        "블랙 새틴 미니 클러치",
        "가방",
        Dark,
        SoftAccent,
        4,
        Thin,
        "satin",
        "satin, mini clutch",
    )
}
fn silver_bag() -> Clothing {
    item(
        "실버 메탈릭 미니 숄더백",
        "가방",
        Bright,
        Accent,
        3,
        Medium,
        "leather",
        "metallic leather, mini",
    )
}
fn chain_bag() -> Clothing {
    item(
        "블랙 미니 체인 숄더백",
        "가방",
        Dark,
        Accent,
        3,
        Medium,
        "leather",
        "leather, chain strap, mini",
    )
}
// 시드에 없는 옷 — 특정 상품에만 맞춘 수정인지 확인한다.
fn holdout_velvet_pants() -> Clothing {
    item(
        "holdout 네이비 벨벳 와이드 팬츠",
        "하의",
        Dark,
        Base,
        3,
        Medium,
        "velvet",
        "velvet, wide leg",
    )
}
fn holdout_metallic_skirt() -> Clothing {
    item(
        "holdout 골드 메탈릭 플리츠 스커트",
        "하의",
        Bright,
        Accent,
        3,
        Thin,
        "polyester",
        "metallic foil, pleated midi",
    )
}
fn holdout_knit() -> Clothing {
    item(
        "holdout 그레이 니트 탑",
        "상의",
        Mid,
        Base,
        2,
        Medium,
        "knit",
        "fine knit",
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
                seasons: vec!["가을".into()],
                texture_worlds: Vec::new(),
            })
            .collect(),
        situation: Some("일상".into()),
    }
}

/// 운영 경로처럼 후보를 만든다 — 무드 반영 채점 → 반복 보정.
fn candidate(idx: usize, items: &[Clothing], recency: i32) -> OutfitCandidate {
    let id_of = |k: SlotKind| items.iter().find(|c| kind(c) == k).map(|c| c.id.clone());
    let mut c = OutfitCandidate::new(
        id_of(SlotKind::Top),
        id_of(SlotKind::Bottom),
        id_of(SlotKind::Outer),
        id_of(SlotKind::Shoes),
        id_of(SlotKind::Bag),
        idx,
    );
    c.style_score =
        mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), Some(StyleGenre::Glam)).score;
    c.recency_penalty = recency;
    c
}

/// 운영과 같은 순서: 날씨 게이트 → 무드 게이트(없으면 그대로) → 오늘의 추천 → 다른 조합 →
/// 안 입은 옷. 돌려주는 것은 고른 후보의 인덱스.
fn select(
    outfits: &[Vec<Clothing>],
    recency: &[i32],
    temperature: f64,
) -> (Option<usize>, Option<usize>, Option<usize>) {
    let wardrobe: Vec<Clothing> = outfits.iter().flatten().cloned().collect();
    let mut cands: Vec<OutfitCandidate> = outfits
        .iter()
        .enumerate()
        .map(|(i, o)| candidate(i, o, recency.get(i).copied().unwrap_or(0)))
        .collect();
    svc::finalize_scores(&mut cands);
    cands.sort_by_key(|c| std::cmp::Reverse(c.final_score));
    let (servable, _) = unfit_gate::keep_wearable_strict(cands, |c| {
        compute_today_fit(&ctx(&outfits[c.ai_candidate_index]), temperature) == TodayFitLevel::Fail
    });
    let reads = |c: &OutfitCandidate| {
        let items: Vec<&Clothing> = outfits[c.ai_candidate_index].iter().collect();
        mood_gate::reads_as(StyleGenre::Glam, &items) == Some(false)
    };
    let (servable, _) = unfit_gate::keep_wearable(servable, reads);
    let todays = svc::select_todays_pick(&servable, temperature, &wardrobe);
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
    let idx =
        |r: &Option<svc::ModeSelectionResult>| r.as_ref().map(|r| r.candidate.ai_candidate_index);
    (idx(&todays), idx(&variation), idx(&dormant))
}

fn codes(items: &[Clothing], mood: Option<StyleGenre>) -> Vec<IssueCode> {
    mood_rules::evaluate_for_mood(&ctx(items), Some("가을"), mood)
        .problems
        .into_iter()
        .map(|p| p.code)
        .collect()
}

/// 1. 글램 상의 + 조용한 아우터는 "이너가 아우터와 경쟁" 으로 깎이지 않는다. 같은 상의라도
///    아우터까지 포인트면 실제로 경쟁하므로 그 조합이 더 낮다.
#[test]
fn glam_top_under_a_quiet_outer_is_not_double_penalised() {
    let quiet = [
        emerald_top(),
        slacks(),
        tailored_jacket(),
        ankle_boots(),
        clutch(),
    ];
    let loud = [
        emerald_top(),
        slacks(),
        velvet_blazer(),
        ankle_boots(),
        clutch(),
    ];
    assert!(
        codes(&quiet, None).contains(&IssueCode::StrongInner),
        "운영 평가는 깎는다"
    );
    let glam = codes(&quiet, Some(StyleGenre::Glam));
    assert!(!glam.contains(&IssueCode::StrongInner), "{glam:?}");
    assert!(codes(&loud, Some(StyleGenre::Glam)).contains(&IssueCode::TooManyAccents));
    let score = |o: &[Clothing]| {
        mood_rules::evaluate_for_mood(&ctx(o), Some("가을"), Some(StyleGenre::Glam)).score
    };
    assert!(score(&quiet) > score(&loud));
    // 14°C: 아우터가 필요한 날. 기본 티 + 같은 아우터와 겨뤄 글램 상의가 오늘의 추천이 된다.
    let plain = vec![tee(), slacks(), tailored_jacket(), ankle_boots(), clutch()];
    let (t, _, _) = select(&[plain, quiet.to_vec()], &[0, 0], 14.0);
    assert_eq!(t, Some(1));
}

/// 2. 기본 상의 + 특징적인 스커트도 유효한 글램이다 — 시드에 없는 벨벳 팬츠로도 같다.
#[test]
fn basic_top_with_a_characteristic_bottom_is_valid_glam() {
    for bottom in [satin_skirt(), holdout_velvet_pants()] {
        let look = vec![tee(), bottom.clone(), ankle_boots(), chain_bag()];
        let items: Vec<&Clothing> = look.iter().collect();
        assert_eq!(
            mood_gate::reads_as(StyleGenre::Glam, &items),
            Some(true),
            "{}",
            bottom.name
        );
        let plain = vec![tee(), denim(), ankle_boots(), silver_bag()];
        let (t, _, _) = select(&[plain, look], &[0, 0], 22.0);
        assert_eq!(t, Some(1), "{}", bottom.name);
    }
}

/// 3. 기본 티셔츠·데님에 실버 가방만 든 조합은 대표 글램이 되지 못한다 — 점수가 더 높아도
///    옷에 글램 요소가 있는 후보가 있으면 그쪽이 뽑힌다.
#[test]
fn a_silver_bag_alone_does_not_win_as_glam() {
    let bag_only = vec![tee(), denim(), ankle_boots(), silver_bag()];
    let skirt = vec![tee(), black_satin_skirt(), ankle_boots(), clutch()];
    let (t, v, _) = select(&[bag_only, skirt], &[0, 0], 22.0);
    assert_eq!(t, Some(1));
    assert_ne!(
        v,
        Some(0),
        "가방만 글램인 후보는 다른 카드로도 나가지 않는다"
    );
}

/// 4. 올블랙이어도 소재·형태 차이가 있으면 "전부 어둡다" 로 깎지 않는다. 무광 기본템만의
///    올블랙은 계속 깎는다.
#[test]
fn all_black_with_shape_or_surface_contrast_is_not_flagged_dark() {
    let sculpted = [
        one_shoulder(),
        black_satin_skirt(),
        ankle_boots(),
        chain_bag(),
    ];
    let matte = [black_tee(), slacks(), ankle_boots(), chain_bag()];
    let dark = |o: &[Clothing]| {
        mood_rules::evaluate_for_mood(&ctx(o), Some("가을"), Some(StyleGenre::Glam))
            .problems
            .iter()
            .any(|p| p.rule == "밝기 밸런스")
    };
    assert!(!dark(&sculpted));
    assert!(dark(&matte));
    // 같은 판단을 다른 조합에도: 시드에 없는 벨벳 팬츠 + 블랙 티
    assert!(!dark(&[
        black_tee(),
        holdout_velvet_pants(),
        ankle_boots(),
        chain_bag()
    ]));
}

/// 5. 가방만 다시 든 새 상하의 조합은 같은 상하의를 다시 낸 조합보다 앞선다.
#[test]
fn reusing_only_the_bag_does_not_push_out_a_new_top_and_bottom() {
    let only_bag = OutfitHistorySummary {
        accessory_used_within_3_days: true,
        ..Default::default()
    };
    let same_pair = OutfitHistorySummary {
        item_used_yesterday: true,
        item_used_within_3_days: true,
        same_top_bottom_within_7_days: true,
        recent_usage_count_7d: 2,
        ..Default::default()
    };
    let (p_bag, p_pair) = (
        calculate_recency_penalty(&only_bag),
        calculate_recency_penalty(&same_pair),
    );
    assert!(p_bag < p_pair);
    assert!(p_bag <= 5, "가방 재사용은 작게만 깎는다: {p_bag}");
    let reused = vec![emerald_top(), slacks(), ankle_boots(), clutch()];
    let fresh = vec![one_shoulder(), black_satin_skirt(), ankle_boots(), clutch()];
    let (t, _, _) = select(&[reused, fresh], &[p_pair, p_bag], 22.0);
    assert_eq!(t, Some(1));
}

/// 반복 보정은 후보끼리의 차이만 남긴다. 모두가 같은 몫을 잃으면 최종 점수가 깎이지 않는다.
#[test]
fn a_penalty_shared_by_every_candidate_is_not_subtracted() {
    let mut cands = vec![
        candidate(0, &[tee(), satin_skirt()], 23),
        candidate(1, &[tee(), black_satin_skirt()], 23),
    ];
    svc::finalize_scores(&mut cands);
    assert!(cands.iter().all(|c| c.recency_penalty == 0));
    assert!(cands.iter().all(|c| c.final_score >= c.style_score));
}

/// 6a. 한 응답의 카드는 서로 겹치지 않는다. 유효한 조합이 둘이면 두 장이다.
#[test]
fn cards_in_one_response_never_repeat() {
    let a = vec![one_shoulder(), black_satin_skirt(), ankle_boots(), clutch()];
    let b = vec![tee(), satin_skirt(), ankle_boots(), chain_bag()];
    let (t, v, d) = select(&[a, b], &[0, 0], 22.0);
    assert!(t.is_some() && v.is_some());
    assert_ne!(t, v);
    assert!(
        d.is_none(),
        "세 번째 조합이 없으면 세 번째 카드도 없다: {d:?}"
    );
}

/// 6b. 날씨에 맞는 후보가 하나도 없으면 아무것도 내지 않는다(후보 부족).
#[test]
fn when_every_candidate_fails_the_weather_nothing_is_served() {
    let thin = vec![one_shoulder(), satin_skirt(), ankle_boots(), clutch()];
    let thin2 = vec![tee(), black_satin_skirt(), ankle_boots(), chain_bag()];
    assert_eq!(compute_today_fit(&ctx(&thin), 3.0), TodayFitLevel::Fail);
    let (t, v, d) = select(&[thin, thin2], &[0, 0], 3.0);
    assert_eq!((t, v, d), (None, None, None));
}

/// 6c. 후보가 하나뿐이면 카드도 하나다 — 같은 후보로 다른 칸을 채우지 않는다.
#[test]
fn a_single_candidate_fills_a_single_card() {
    let only = vec![one_shoulder(), black_satin_skirt(), ankle_boots(), clutch()];
    let (t, v, d) = select(&[only], &[0], 22.0);
    assert_eq!((t, v, d), (Some(0), None, None));
}

/// 7. 하의가 유일한 포인트일 때 하의라는 이유로 깎지 않는다(시드에 없는 포인트 스커트).
#[test]
fn a_statement_bottom_is_not_penalised_for_being_a_bottom() {
    let look = [
        holdout_knit(),
        holdout_metallic_skirt(),
        ankle_boots(),
        clutch(),
    ];
    let glam = codes(&look, Some(StyleGenre::Glam));
    assert!(codes(&look, None).contains(&IssueCode::SlotRoleMismatch));
    assert!(!glam.contains(&IssueCode::SlotRoleMismatch), "{glam:?}");
}

/// 8. 신발의 날씨: 확실히 맞지 않는 것과 경계를 나눈다.
#[test]
fn shoe_weather_separates_clear_misfits_from_borderline() {
    let mut knee = ankle_boots();
    knee.name = "블랙 레더 니하이 롱부츠".into();
    knee.sub_category = Some("롱부츠".into());
    let mut ankle = ankle_boots();
    ankle.sub_category = Some("앵클부츠".into());
    let mut sandal = ankle_boots();
    sandal.name = "실버 메탈릭 스트랩 샌들".into();
    sandal.sub_category = Some("샌들".into());
    assert_eq!(shoe_weather(&knee, 27.0), ShoeWeather::Unsuitable);
    assert_eq!(shoe_weather(&knee, 18.0), ShoeWeather::Fine);
    assert_eq!(shoe_weather(&ankle, 27.0), ShoeWeather::Borderline);
    assert_eq!(shoe_weather(&ankle, 22.0), ShoeWeather::Fine);
    assert_eq!(shoe_weather(&sandal, 5.0), ShoeWeather::Unsuitable);
    assert_eq!(shoe_weather(&sandal, 11.0), ShoeWeather::Borderline);
    assert_eq!(shoe_weather(&sandal, 25.0), ShoeWeather::Fine);
}

/// 다른 무드의 채점은 그대로다 — 무드 반영 평가가 운영 평가와 같다. 엣지는 자기 규칙이
/// 있다(`tests/edgy_selection.rs`).
#[test]
fn other_moods_are_scored_exactly_as_before() {
    let look = [
        emerald_top(),
        slacks(),
        tailored_jacket(),
        ankle_boots(),
        silver_bag(),
    ];
    let base = engine::evaluate(&ctx(&look), Some("가을")).score;
    for g in StyleGenre::ALL
        .iter()
        .filter(|g| !matches!(g, StyleGenre::Glam | StyleGenre::Edgy))
    {
        assert_eq!(
            mood_rules::evaluate_for_mood(&ctx(&look), Some("가을"), Some(*g)).score,
            base,
            "{g}"
        );
    }
    assert_eq!(
        mood_rules::evaluate_for_mood(&ctx(&look), Some("가을"), None).score,
        base
    );
}

/// 홈의 '일상' 은 글램 격식 상한을 넓히지만, 사용자가 고른 출근은 그대로 깎는다.
#[test]
fn explicit_office_keeps_its_formality_rule() {
    let look = [tee(), denim(), ankle_boots(), clutch()]; // 평균 격식 2.5 — 출근엔 낮다
    let mut office = ctx(&look);
    office.situation = Some("출근".into());
    let e = mood_rules::evaluate_for_mood(&office, Some("가을"), Some(StyleGenre::Glam));
    assert!(
        e.problems
            .iter()
            .any(|p| p.code == IssueCode::FormalitySituationMismatch)
    );
}

fn coat(
    name: &str,
    role: Role,
    material: &str,
    sub: &str,
    keywords: &str,
    sat: Saturation,
) -> Clothing {
    let mut c = item(name, "아우터", Dark, role, 4, Thick, material, keywords);
    c.sub_category = Some(sub.into());
    c.saturation = Some(sat);
    c.weight = Some(Weight::Mid);
    c
}
fn fur_half_coat() -> Clothing {
    let mut c = coat(
        "에스프레소 브라운 페이크퍼 하프코트",
        Accent,
        "faux fur",
        "하프코트",
        "plush faux fur, hip-length half coat, insulated lining",
        Saturation::Mid,
    );
    c.weight = Some(Weight::Heavy);
    c
}
fn hourglass_coat() -> Clothing {
    coat(
        "블랙 아워글라스 롱 울 코트",
        Structural,
        "wool",
        "롱코트",
        "heavy wool blend, hourglass silhouette, nipped waist, mid-calf length, insulated lining",
        Saturation::Low,
    )
}
fn burgundy_coat() -> Clothing {
    coat(
        "딥 버건디 벨티드 롱 울 코트",
        Accent,
        "wool",
        "롱코트",
        "heavy matte wool, wide shawl collar, self-fabric belt, insulated lining",
        Saturation::High,
    )
}
fn plain_wool_coat() -> Clothing {
    let mut c = coat(
        "크림 울혼방 롱 코트",
        Structural,
        "wool",
        "코트",
        "wool blend, long, drop shoulder",
        Saturation::Low,
    );
    c.tone = Some(Bright);
    c
}
fn mini_skirt() -> Clothing {
    item(
        "블랙 드레이프 랩 미니스커트",
        "하의",
        Dark,
        Base,
        3,
        Medium,
        "polyester",
        "matte crepe, draped wrap front, mini length",
    )
}
fn slim_knit() -> Clothing {
    item(
        "블랙 리브드 슬림 니트",
        "상의",
        Dark,
        Base,
        2,
        Medium,
        "knit",
        "ribbed knit, slim fit",
    )
}

/// 겨울 코트도 글램이어야 한다. 퍼·아워글라스·선명한 색 코트는 글램 요소이고, 단정한
/// 무광 울 코트는 아니다.
#[test]
fn a_winter_coat_counts_as_glam_only_when_it_is_glam() {
    for c in [fur_half_coat(), hourglass_coat(), burgundy_coat()] {
        assert!(mood_gate::is_glam_piece(&c), "{}", c.name);
    }
    assert!(!mood_gate::is_glam_piece(&plain_wool_coat()));
    // 코트가 주인공인 글램 — 담백한 니트·슬랙스 위에 퍼 코트. 단정한 코트로 바꾸면 글램이 아니다.
    let hero = vec![
        slim_knit(),
        slacks(),
        fur_half_coat(),
        ankle_boots(),
        clutch(),
    ];
    let plain = vec![
        slim_knit(),
        slacks(),
        plain_wool_coat(),
        ankle_boots(),
        clutch(),
    ];
    let (t, _, _) = select(&[plain, hero], &[0, 0], 2.0);
    assert_eq!(t, Some(1));
}

fn knee_boots() -> Clothing {
    let mut c = item(
        "블랙 스웨이드 니하이 롱부츠",
        "신발",
        Dark,
        Connector,
        3,
        Medium,
        "suede",
        "suede, knee-high, slim shaft",
    );
    c.sub_category = Some("롱부츠".into());
    c
}

/// 한겨울 글램은 일반 외출 기준이다. 상체는 보온 안감이 있는 두꺼운 코트를 여며 입으면
/// 얇은 탑도 괜찮지만, 하체는 따로 본다 — 미니에 앵클부츠면 코트가 무릎을 덮어도 다리가
/// 드러나 0°C 에서 실패다. 미니 자체를 막지는 않는다: 니하이 부츠에 롱코트면 경계로 남고,
/// 8°C 에서는 니하이 부츠만으로 통과다. 바지면 같은 상체·코트로 통과다.
#[test]
fn a_mini_in_winter_needs_the_legs_covered_too() {
    let fit = |o: &[Clothing], t| compute_today_fit(&ctx(o), t);
    let mini_ankle = vec![
        one_shoulder(),
        mini_skirt(),
        hourglass_coat(),
        ankle_boots(),
        clutch(),
    ];
    let mini_knee = vec![
        one_shoulder(),
        mini_skirt(),
        hourglass_coat(),
        knee_boots(),
        clutch(),
    ];
    let pants = vec![
        one_shoulder(),
        slacks(),
        hourglass_coat(),
        ankle_boots(),
        clutch(),
    ];
    let mini_no_coat = vec![one_shoulder(), mini_skirt(), knee_boots(), clutch()];
    assert_eq!(fit(&mini_ankle, 0.0), TodayFitLevel::Fail);
    assert_eq!(fit(&mini_knee, 2.0), TodayFitLevel::Borderline);
    assert_eq!(fit(&pants, 2.0), TodayFitLevel::Pass);
    assert_eq!(fit(&mini_no_coat, 2.0), TodayFitLevel::Fail);
    assert_eq!(fit(&mini_knee, 8.0), TodayFitLevel::Pass);
    assert_eq!(fit(&mini_ankle, 8.0), TodayFitLevel::Borderline);

    // 선택: 2°C 에 미니·앵클부츠와 같은 상체·코트의 바지가 있으면 바지를 고른다.
    let (t, _, _) = select(&[mini_ankle.clone(), pants], &[0, 0], 2.0);
    assert_eq!(t, Some(1));
    // 맞는 후보가 하나도 없으면 실패한 후보를 대신 내지 않는다.
    let (t, _, _) = select(&[mini_ankle, mini_no_coat], &[0, 0], 0.0);
    assert_eq!(t, None);
}

fn rider() -> Clothing {
    let mut c = item(
        "블랙 레더 라이더 자켓",
        "아우터",
        Dark,
        Structural,
        3,
        Medium,
        "leather",
        "leather, cropped biker, silver hardware",
    );
    c.sub_category = Some("라이더자켓".into());
    c.weight = Some(Weight::Heavy);
    c
}
fn sandals() -> Clothing {
    let mut c = item(
        "실버 메탈릭 스트랩 샌들",
        "신발",
        Bright,
        Accent,
        4,
        Thin,
        "leather",
        "metallic leather, thin straps",
    );
    c.sub_category = Some("샌들".into());
    c
}

/// 상체가 경계여도 하체 판정은 따로 본다. 2°C 에 니트 + 일반 라이더(상체 경계)에 미니면
/// 니하이 부츠를 신어도 무릎을 덮는 코트가 없어 실패다 — 상체 경계에서 판정이 먼저 끝나
/// 오늘의 추천에 올랐던 조합(2026-10-17 실측). 같은 상체에 슬랙스면 경계로 남는다.
#[test]
fn a_borderline_torso_does_not_skip_the_leg_check() {
    let fit = |o: &[Clothing], t| compute_today_fit(&ctx(o), t);
    let mini = [
        slim_knit(),
        mini_skirt(),
        rider(),
        knee_boots(),
        chain_bag(),
    ];
    let pants = [slim_knit(), slacks(), rider(), ankle_boots(), chain_bag()];
    assert_eq!(fit(&mini, 2.0), TodayFitLevel::Fail);
    assert_eq!(fit(&pants, 2.0), TodayFitLevel::Borderline);
    assert_eq!(fit(&mini, 8.0), TodayFitLevel::Pass);
}

/// 발이 드러나는 샌들은 하의와 무관하게 본다. 8°C 면 경계, 5°C 면 실패, 15°C 면 통과다.
#[test]
fn open_shoes_are_judged_on_cold_days() {
    let fit = |t| {
        compute_today_fit(
            &ctx(&[
                slim_knit(),
                satin_skirt(),
                hourglass_coat(),
                sandals(),
                clutch(),
            ]),
            t,
        )
    };
    assert_eq!(fit(8.0), TodayFitLevel::Borderline);
    assert_eq!(fit(5.0), TodayFitLevel::Fail);
    assert_eq!(fit(15.0), TodayFitLevel::Pass);
}

/// 착장마다 중심 소재는 하나 — 퍼 코트와 새틴 강한 상의를 함께 강조하면 깎는다. 같은 강한
/// 상의라도 몸의 선으로 받치는 아워글라스 코트면 깎지 않는다.
#[test]
fn a_statement_coat_and_a_statement_top_compete() {
    let both = [
        emerald_top(),
        slacks(),
        fur_half_coat(),
        ankle_boots(),
        clutch(),
    ];
    let supported = [
        emerald_top(),
        slacks(),
        hourglass_coat(),
        ankle_boots(),
        clutch(),
    ];
    assert!(codes(&both, Some(StyleGenre::Glam)).contains(&IssueCode::TooManyAccents));
    let s = |o: &[Clothing]| {
        mood_rules::evaluate_for_mood(&ctx(o), Some("겨울"), Some(StyleGenre::Glam)).score
    };
    assert!(s(&supported) > s(&both));
}

/// 길이가 보온에 들어간다 — 같은 두께라도 크롭 재킷은 롱코트보다 덜 따뜻하다.
#[test]
fn a_cropped_jacket_is_less_warm_than_a_long_coat() {
    let mut crop = coat(
        "아이보리 페이크퍼 크롭 재킷",
        Accent,
        "faux fur",
        "크롭자켓",
        "dense short-pile faux fur, cropped at the waist",
        Saturation::Low,
    );
    crop.tone = Some(Bright);
    let thin_top = tee();
    let long = vec![
        thin_top.clone(),
        slacks(),
        hourglass_coat(),
        ankle_boots(),
        clutch(),
    ];
    let short = vec![thin_top, slacks(), crop, ankle_boots(), clutch()];
    // 0°C: 롱코트는 통과, 크롭 재킷은 보온이 모자란다. 니트 위의 크롭 재킷은 통과한다.
    assert_eq!(compute_today_fit(&ctx(&long), 0.0), TodayFitLevel::Pass);
    assert_ne!(compute_today_fit(&ctx(&short), 0.0), TodayFitLevel::Pass);
    let mut knit_crop = short.clone();
    knit_crop[0] = holdout_knit();
    assert_ne!(
        compute_today_fit(&ctx(&knit_crop), 0.0),
        TodayFitLevel::Fail
    );
}
