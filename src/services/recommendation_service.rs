use std::collections::HashSet;

use anyhow::Result;
use sqlx::MySqlPool;

use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::models::clothing::Clothing;
use crate::models::recommendation::{OutfitCandidate, ScoringDetail};
use crate::models::style_vocab::{Role, Style, Thickness, Tone, Weight};
use crate::services::recommendation_diversity::{
    calculate_diversity_bonus, calculate_dormant_bonus, calculate_recency_penalty,
};

pub async fn rerank_candidates_with_history(
    pool: &MySqlPool,
    user_id: &str,
    mut candidates: Vec<OutfitCandidate>,
) -> Result<Vec<OutfitCandidate>> {
    for candidate in &mut candidates {
        let history = RecommendationHistoryRepo::summarize_candidate_recency(
            pool,
            user_id,
            candidate.top_id.as_deref(),
            candidate.bottom_id.as_deref(),
            candidate.outer_id.as_deref(),
            candidate.shoes_id.as_deref(),
            candidate.bag_id.as_deref(),
        )
        .await?;

        candidate.recency_penalty = calculate_recency_penalty(&history);
        candidate.diversity_bonus = calculate_diversity_bonus(&history);
    }

    finalize_scores(&mut candidates);

    candidates.sort_by(|a, b| {
        b.final_score
            .cmp(&a.final_score)
            .then_with(|| b.style_score.cmp(&a.style_score))
    });

    Ok(candidates)
}

/// 반복 감점·다양성 가점을 얹어 최종 점수를 낸다.
///
/// 반복 감점은 후보끼리의 차이로만 쓴다. 다섯 후보가 모두 같은 가방·신발을 다시 쓰는
/// 작은 옷장에서는 모두가 같은 몫을 잃고 순서는 그대로인데, 최종 점수만 낮아져 좋은
/// 코디도 "아쉬워요" 로 보였다. 모두에게 공통인 몫을 빼고 남는 차이만 반영한다.
pub fn finalize_scores(candidates: &mut [OutfitCandidate]) {
    let shared = candidates
        .iter()
        .map(|c| c.recency_penalty)
        .min()
        .unwrap_or(0);
    for candidate in candidates.iter_mut() {
        candidate.recency_penalty -= shared;
        candidate.final_score = candidate.style_score - candidate.recency_penalty
            + candidate.diversity_bonus
            - candidate.weather_penalty;
    }
}

pub fn select_recommendation(candidates: &[OutfitCandidate]) -> Option<OutfitCandidate> {
    if candidates.is_empty() {
        return None;
    }
    let top: Vec<_> = candidates.iter().take(3).cloned().collect();
    let filtered: Vec<_> = top.into_iter().filter(|c| c.style_score >= 70).collect();
    if filtered.is_empty() {
        return candidates.first().cloned();
    }
    filtered.into_iter().next()
}

// ═══════════════════════════════════════════════════════
// Today 적합도 평가 시스템
// ═══════════════════════════════════════════════════════

/// 후보의 Today 적합도 — 3가지 축으로 평가
struct TodayFitness {
    temp_suitable: bool, // 온도 적합
    has_contrast: bool,  // 대비 충분
    has_structure: bool, // 구조감 있음
    penalty: i32,        // 총 감점
}

impl TodayFitness {
    fn pass_count(&self) -> u8 {
        self.temp_suitable as u8 + self.has_contrast as u8 + self.has_structure as u8
    }

    /// 최소 2/3 조건 통과해야 Today 후보 자격
    fn is_qualified(&self) -> bool {
        self.pass_count() >= 2
    }
}

fn find_clothing<'a>(id: &Option<String>, clothes: &'a [Clothing]) -> Option<&'a Clothing> {
    id.as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id))
}

fn evaluate_today_fitness(
    c: &OutfitCandidate,
    temperature: f64,
    clothes: &[Clothing],
) -> TodayFitness {
    let top = find_clothing(&c.top_id, clothes);
    let bottom = find_clothing(&c.bottom_id, clothes);
    let outer = find_clothing(&c.outer_id, clothes);

    let mut penalty = 0;

    // ─── 1. 온도 적합성 ───
    let is_light_top =
        top.is_some_and(|t| t.weight == Some(Weight::Light) || t.thickness == Thickness::Thin);
    let has_outer = outer.is_some();

    let temp_suitable = if temperature <= 13.0 {
        // 13도 이하: 반팔 단독 reject
        if is_light_top && !has_outer {
            penalty += 20;
            false
        } else {
            true
        }
    } else if temperature <= 18.0 {
        // 13~18도: 반팔 허용하되 아우터/긴팔 선호
        if is_light_top && !has_outer {
            penalty += 8;
            false // soft fail — 감점은 있지만 다른 조건으로 보완 가능
        } else {
            true
        }
    } else {
        true // 18도 이상: 제한 없음
    };

    // 경계에 있는 신발(27°C 이상의 부츠, 13°C 미만의 샌들)은 조금 민다. 확실히 맞지
    // 않는 신발은 후보 단계(`shortlist::shoe_weather`)에서 이미 빠졌다.
    let shoes = find_clothing(&c.shoes_id, clothes);
    if shoes.is_some_and(|s| {
        crate::services::shortlist::shoe_weather(s, temperature)
            == crate::services::shortlist::ShoeWeather::Borderline
    }) {
        penalty += 5;
    }

    // ─── 2. 대비 충분성 ───
    let top_tone = top.and_then(|t| t.tone);
    let bottom_tone = bottom.and_then(|t| t.tone);
    let top_temp = top.and_then(|t| t.color_temperature.as_deref());
    let bottom_temp = bottom.and_then(|t| t.color_temperature.as_deref());

    let has_contrast = match (top_tone, bottom_tone) {
        (Some(tt), Some(bt)) if tt == bt => {
            // 같은 밝기 → 색온도라도 달라야 함
            let same_color_temp = top_temp.is_some() && top_temp == bottom_temp;
            if same_color_temp {
                // 완전 동일 톤+색온도: 강한 감점
                penalty += 12;
                false
            } else if tt == Tone::Bright {
                // 둘 다 밝음 (색온도는 다름): 약한 감점
                let outer_helps = outer
                    .and_then(|o| o.tone)
                    .is_some_and(|t| t == Tone::Dark || t == Tone::Mid);
                if outer_helps {
                    true
                } else {
                    penalty += 8;
                    false
                }
            } else {
                true // 둘 다 어두움/중간이지만 색온도 다름 → OK
            }
        }
        _ => true, // 데이터 없음 → 패스
    };

    // ─── 3. 구조감 ───
    let bottom_structured = bottom.is_some_and(|b| {
        b.role == Some(Role::Structural)
            || b.style == Some(Style::Formal)
            || b.weight == Some(Weight::Heavy)
    });
    let outer_structured = outer.is_some_and(|o| {
        matches!(
            o.role,
            Some(Role::Structural) | Some(Role::SoftAccent) | Some(Role::Accent)
        )
    });
    let top_is_structure = top.is_some_and(|t| t.role == Some(Role::Structural));

    let has_structure = bottom_structured || outer_structured || top_is_structure;
    if !has_structure {
        penalty += 7;
    }

    // ─── 4. 역할 밸런스 (포인트 과다 / 올 베이직) ───
    let roles: Vec<Role> = [top, bottom, outer]
        .iter()
        .filter_map(|item| item.and_then(|i| i.role))
        .collect();

    let accent_count = roles
        .iter()
        .filter(|r| **r == Role::Accent || **r == Role::SoftAccent)
        .count();
    if accent_count >= 2 {
        penalty += 5;
    }

    TodayFitness {
        temp_suitable,
        has_contrast,
        has_structure,
        penalty,
    }
}

// ═══════════════════════════════════════════════════════
// Mode-specific sequential selection
// ═══════════════════════════════════════════════════════

pub struct ModeSelectionResult {
    pub candidate: OutfitCandidate,
    pub scoring: ScoringDetail,
}

/// Step 1: 오늘의 추천 — 적합도 게이트 + 최고 점수
///
/// 1·2차의 "스타일 70점 이상" 은 날씨 경계 감점(`weather_penalty`)을 뺀 점수로 본다.
/// 예전에는 스타일 점수만 봐서, 2°C 에 경계인 니트 + 일반 라이더(스타일 79)가 1차를 혼자
/// 통과해 최종 점수가 더 높은 보온 안감 롱코트 조합(스타일 65, 통과)을 이겼다 — 경계
/// 감점이 순위에 닿지 못했다(2026-10-17 엣지 실측).
/// 오늘의 추천 1·2차의 스타일 기준(날씨 경계 감점을 뺀 값).
const TODAY_STYLE_BAR: i32 = 70;

/// 오늘의 추천의 자격 단계. `select_todays_pick` 과 같은 순서다 — 1: 스타일 기준 + 적합도
/// 2/3 통과, 2: 스타일 기준만, 3: 나머지. 작을수록 앞선다.
fn today_tier(c: &OutfitCandidate, temperature: f64, clothes: &[Clothing]) -> u8 {
    let good = c.style_score - c.weather_penalty >= TODAY_STYLE_BAR;
    match (
        good,
        evaluate_today_fitness(c, temperature, clothes).is_qualified(),
    ) {
        (true, true) => 1,
        (true, false) => 2,
        _ => 3,
    }
}

pub fn select_todays_pick(
    candidates: &[OutfitCandidate],
    temperature: f64,
    clothes: &[Clothing],
) -> Option<ModeSelectionResult> {
    // 각 후보에 적합도 평가
    let scored: Vec<(&OutfitCandidate, TodayFitness, i32)> = candidates
        .iter()
        .map(|c| {
            let fitness = evaluate_today_fitness(c, temperature, clothes);
            let adjusted = c.final_score - fitness.penalty;
            (c, fitness, adjusted)
        })
        .collect();

    let good = |c: &OutfitCandidate| c.style_score - c.weather_penalty >= TODAY_STYLE_BAR;
    // 1차: style >= 70 + 2/3 조건 통과 후보
    let pick = scored
        .iter()
        .filter(|(c, f, _)| good(c) && f.is_qualified())
        .max_by_key(|(_, _, adj)| *adj)
        // 2차: style >= 70 + 감점 적용
        .or_else(|| {
            scored
                .iter()
                .filter(|(c, _, _)| good(c))
                .max_by_key(|(_, _, adj)| *adj)
        })
        // 3차: 전체에서 최고점
        .or_else(|| scored.iter().max_by_key(|(_, _, adj)| *adj));

    pick.map(|(c, fitness, adjusted)| ModeSelectionResult {
        scoring: ScoringDetail {
            style_score: c.style_score,
            recency_penalty: c.recency_penalty + fitness.penalty,
            diversity_bonus: c.diversity_bonus,
            dormant_bonus: 0,
            weather_penalty: c.weather_penalty,
            final_score: *adjusted,
        },
        candidate: (*c).clone(),
    })
}

/// Step 2: 다른 조합 — Today와 상의+하의가 다른 후보 중 최고 점수
pub fn select_variation(
    candidates: &[OutfitCandidate],
    todays_pick: &OutfitCandidate,
) -> Option<ModeSelectionResult> {
    // 1차: 상의+하의 모두 다른 후보
    let different_pair: Vec<_> = candidates
        .iter()
        .filter(|c| !has_same_top_bottom(c, todays_pick))
        .collect();

    if let Some(best) = pick_best_variation(&different_pair) {
        return Some(best);
    }

    // 2차: 상의 또는 하의 중 하나라도 다른 후보
    let different_one: Vec<_> = candidates
        .iter()
        .filter(|c| !is_same_outfit(c, todays_pick))
        .collect();

    if let Some(best) = pick_best_variation(&different_one) {
        return Some(best);
    }

    // 더 고를 것이 없다. 예전에는 "두 번째 후보" 를 그대로 냈는데, Today 가 첫 번째가
    // 아닐 때 그 후보가 Today 와 같은 코디였다. 유효한 조합이 하나면 카드도 한 장이다.
    None
}

/// Step 3: 안 입은 옷 — 휴면 아이템을 포함하면서 Today와 top+bottom이 다르고
/// 품질 기준을 통과하는 후보가 있을 때만 추천. 없으면 None (옵셔널 카드)
///
/// 앞서 고른 카드 **전부**와 상하의가 달라야 한다. 예전에는 Today 만 봐서 '다른 조합' 과
/// 같은 코디가 이 자리에 다시 나왔다(2026-10-17 연속 요청 4회 중 2회).
pub fn select_dormant(
    candidates: &[OutfitCandidate],
    dormant_ids: &HashSet<String>,
    chosen: &[&OutfitCandidate],
) -> Option<ModeSelectionResult> {
    let pool: Vec<_> = candidates
        .iter()
        .filter(|c| contains_dormant_item(c, dormant_ids))
        .filter(|c| !chosen.iter().any(|p| has_same_top_bottom(c, p)))
        .collect();

    pick_best_dormant(&pool, dormant_ids)
}

// ─── Helper functions ───

/// 모르는 색을 가리키는 값. 색군 판정(`outfit_scorer::color_group`)은 이런 값도 "other" 로
/// 돌려줘서, 그대로 두면 "미상" 이 유채색으로 셌다.
const UNKNOWN_COLOURS: [&str; 10] = [
    "미상",
    "모름",
    "알수없음",
    "기타",
    "unknown",
    "none",
    "n/a",
    "na",
    "-",
    "?",
];

/// 무채색·뉴트럴. 색군 판정(`outfit_scorer::color_group`)이 모르는 이름이다 — 영문 표기,
/// 카멜·모카 같은 갈색 계열, 실버·샴페인 같은 금속색. 카키는 색군 판정이 올리브로 보므로 여기 넣지 않는다.
const NEUTRAL_COLOUR_WORDS: [&str; 19] = [
    "black",
    "white",
    "gray",
    "grey",
    "charcoal",
    "ivory",
    "cream",
    "beige",
    "brown",
    "navy",
    "denim",
    "tan",
    "camel",
    "카멜",
    "모카",
    "에스프레소",
    "토프",
    "실버",
    "샴페인",
];

/// 채도가 비어 있을 때 색으로 인정하는 이름. 저장된 채도가 있으면 그 값을 따르고, 없으면
/// 이름이 분명히 유채색일 때만 센다 — 채도를 지어내지 않는다.
const CHROMATIC_COLOUR_WORDS: [&str; 34] = [
    "레드",
    "빨강",
    "체리",
    "버건디",
    "와인",
    "핑크",
    "코럴",
    "오렌지",
    "머스타드",
    "옐로",
    "노랑",
    "라임",
    "그린",
    "초록",
    "민트",
    "틸",
    "청록",
    "블루",
    "파랑",
    "코발트",
    "페트롤",
    "바이올렛",
    "보라",
    "퍼플",
    "플럼",
    "라벤더",
    "red",
    "pink",
    "orange",
    "yellow",
    "green",
    "blue",
    "purple",
    "violet",
];

/// 유채색 옷인가. 보수적으로 본다:
/// - 색이 비었거나 모른다는 값("미상", "unknown" 등)이면 아니다.
/// - 검정·차콜·그레이·화이트·베이지·브라운·네이비(데님) 계열이면 한국어든 영문이든 아니다.
/// - 채도가 낮다고 적혀 있으면 아니다. 중간·높음이면 유채색이다.
/// - 채도가 비어 있으면 이름이 분명한 유채색(레드·머스타드·코발트 등)일 때만이다.
pub fn is_chromatic(c: &Clothing) -> bool {
    use crate::models::style_vocab::Saturation;
    let Some(color) = c.color.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let lower = color.to_lowercase();
    let compact: String = lower.chars().filter(|ch| !ch.is_whitespace()).collect();
    if UNKNOWN_COLOURS.contains(&compact.as_str()) {
        return false;
    }
    if NEUTRAL_COLOUR_WORDS.iter().any(|w| lower.contains(w))
        || matches!(
            crate::services::outfit_scorer::color_group(color),
            "dark" | "gray" | "cream" | "beige" | "brown" | "navy"
        )
    {
        return false;
    }
    match c.saturation {
        Some(Saturation::Low) => false,
        Some(_) => true,
        None => CHROMATIC_COLOUR_WORDS.iter().any(|w| lower.contains(w)),
    }
}

/// 착장에서 눈에 보이는 유채색 옷. 아우터의 색은 늘 보인다. 상의의 색은 아우터가 없거나,
/// 코트를 여며 입지 않는 기온(`CLOSED_COAT_BELOW_C` 이상)일 때만 보인다 — 2°C 에 여민 코트
/// 안의 바이올렛 니트는 사진에서도 거의 보이지 않는다. 가방·신발의 색은 세지 않는다.
pub fn visible_color<'a>(
    c: &OutfitCandidate,
    clothes: &'a [Clothing],
    temperature: f64,
) -> Option<&'a Clothing> {
    let outer = find_clothing(&c.outer_id, clothes);
    if let Some(o) = outer.filter(|o| is_chromatic(o)) {
        return Some(o);
    }
    let top_shows =
        outer.is_none() || temperature >= crate::services::serving_ranker::CLOSED_COAT_BELOW_C;
    find_clothing(&c.top_id, clothes).filter(|t| top_shows && is_chromatic(t))
}

/// 엣지 카드의 색. 오늘의 추천과 다른 조합이 모두 무채색으로만 보일 때, 순위가 가까운 컬러
/// 후보를 고른다(2026-10-17 사용자 요청 — 엣지 추천에서 색이 다양하게 나오게).
///
/// 점수를 바꾸지 않고, 가까운 후보 사이에서만 고른다:
/// - 오늘의 추천: 같은 판정(적합도 감점 반영) 최종 점수가 `TODAY_COLOR_MARGIN` 안이고, 오늘의
///   추천을 고른 자격 단계(`today_tier`)와 같거나 더 높은 단계일 때만.
/// - 다른 조합: 같은 기준의 점수가 `VARIATION_COLOR_MARGIN` 안이면.
/// - 둘 다 스타일 점수에서 날씨 경계 감점을 뺀 값이 `COLOR_QUALITY_FLOOR` 이상이어야 한다.
///
/// 컬러라는 이유만으로 품질이 낮은 후보를 올리지 않는다. 날씨에 맞지 않는 후보는 이 함수에
/// 들어오기 전에 이미 빠졌다(`servable`). 맞는 컬러 후보가 없으면 무채색 카드 그대로다.
pub fn prefer_visible_color(
    servable: &[OutfitCandidate],
    todays: Option<ModeSelectionResult>,
    variation: Option<ModeSelectionResult>,
    temperature: f64,
    clothes: &[Clothing],
) -> (Option<ModeSelectionResult>, Option<ModeSelectionResult>) {
    let Some(mut todays) = todays else {
        return (None, variation);
    };
    let mut variation = variation;
    let shows = |c: &OutfitCandidate| visible_color(c, clothes, temperature).is_some();
    let good = |c: &OutfitCandidate| c.style_score - c.weather_penalty >= COLOR_QUALITY_FLOOR;

    if !shows(&todays.candidate) {
        // 오늘의 추천을 고른 자격 단계보다 낮은 단계의 후보로는 바꾸지 않는다. 최종 점수가
        // 가까워도 스타일 70 이상·적합도 2/3 를 갖춘 후보를 그렇지 않은 컬러 후보로 바꾸지
        // 않는다(Codex 검토 — 스타일 80·반복 −20 무채색을 스타일 65 컬러로 바꾸던 문제).
        let winner_tier = today_tier(&todays.candidate, temperature, clothes);
        let pick = servable
            .iter()
            .filter(|c| !is_same_outfit(c, &todays.candidate) && shows(c) && good(c))
            .filter(|c| today_tier(c, temperature, clothes) <= winner_tier)
            .map(|c| {
                let fitness = evaluate_today_fitness(c, temperature, clothes);
                (c, fitness.penalty, c.final_score - fitness.penalty)
            })
            .filter(|(_, _, adj)| *adj >= todays.scoring.final_score - TODAY_COLOR_MARGIN)
            .max_by_key(|(_, _, adj)| *adj);
        if let Some((c, penalty, adjusted)) = pick {
            todays = ModeSelectionResult {
                scoring: ScoringDetail {
                    style_score: c.style_score,
                    recency_penalty: c.recency_penalty + penalty,
                    diversity_bonus: c.diversity_bonus,
                    dormant_bonus: 0,
                    weather_penalty: c.weather_penalty,
                    final_score: adjusted,
                },
                candidate: c.clone(),
            };
            variation = select_variation(servable, &todays.candidate);
        }
    }

    let variation_shows = variation.as_ref().is_some_and(|v| shows(&v.candidate));
    if !shows(&todays.candidate) && !variation_shows {
        let key = |c: &OutfitCandidate| {
            c.style_score + c.diversity_bonus * 3 - c.recency_penalty - c.weather_penalty
        };
        let floor = variation
            .as_ref()
            .map_or(i32::MIN, |v| key(&v.candidate) - VARIATION_COLOR_MARGIN);
        // 다른 조합도 기존 단계(`pick_best_variation` 의 스타일 68 이상)를 지킨다.
        let style_bar = variation
            .as_ref()
            .filter(|v| v.candidate.style_score >= VARIATION_STYLE_BAR)
            .map_or(i32::MIN, |_| VARIATION_STYLE_BAR);
        if let Some(c) = servable
            .iter()
            .filter(|c| !has_same_top_bottom(c, &todays.candidate) && shows(c) && good(c))
            .filter(|c| key(c) >= floor && c.style_score >= style_bar)
            .max_by_key(|c| key(c))
        {
            variation = Some(ModeSelectionResult {
                scoring: build_variation_scoring(c),
                candidate: c.clone(),
            });
        }
    }
    (Some(todays), variation)
}

/// 오늘의 추천에서 컬러 후보를 고를 수 있는 점수 차이.
const TODAY_COLOR_MARGIN: i32 = 5;
/// 다른 조합에서 컬러 후보를 고를 수 있는 점수 차이.
const VARIATION_COLOR_MARGIN: i32 = 10;
/// 다른 조합이 먼저 고르는 스타일 점수.
const VARIATION_STYLE_BAR: i32 = 68;
/// 컬러 후보가 갖춰야 할 스타일 점수(날씨 경계 감점을 뺀 값).
const COLOR_QUALITY_FLOOR: i32 = 60;

pub fn has_same_top_bottom(a: &OutfitCandidate, b: &OutfitCandidate) -> bool {
    a.top_id.is_some()
        && a.top_id == b.top_id
        && a.bottom_id.is_some()
        && a.bottom_id == b.bottom_id
}

pub fn is_same_outfit(a: &OutfitCandidate, b: &OutfitCandidate) -> bool {
    a.top_id == b.top_id && a.bottom_id == b.bottom_id && a.outer_id == b.outer_id
}

pub fn contains_dormant_item(candidate: &OutfitCandidate, dormant_ids: &HashSet<String>) -> bool {
    [
        &candidate.top_id,
        &candidate.bottom_id,
        &candidate.outer_id,
        &candidate.shoes_id,
        &candidate.bag_id,
    ]
    .iter()
    .any(|id| id.as_ref().is_some_and(|id| dormant_ids.contains(id)))
}

fn pick_best_variation(pool: &[&OutfitCandidate]) -> Option<ModeSelectionResult> {
    pool.iter()
        .filter(|c| c.style_score >= VARIATION_STYLE_BAR)
        .max_by_key(|c| {
            c.style_score + c.diversity_bonus * 3 - c.recency_penalty - c.weather_penalty
        })
        .or_else(|| pool.first())
        .map(|c| ModeSelectionResult {
            scoring: build_variation_scoring(c),
            candidate: (*c).clone(),
        })
}

fn pick_best_dormant(
    pool: &[&OutfitCandidate],
    dormant_ids: &HashSet<String>,
) -> Option<ModeSelectionResult> {
    pool.iter()
        .filter(|c| c.style_score >= 70)
        .max_by_key(|c| {
            let dormant = calculate_dormant_bonus(c, dormant_ids);
            c.style_score + dormant - c.weather_penalty
        })
        .map(|c| {
            let dormant = calculate_dormant_bonus(c, dormant_ids);
            ModeSelectionResult {
                scoring: ScoringDetail {
                    style_score: c.style_score,
                    recency_penalty: c.recency_penalty,
                    diversity_bonus: c.diversity_bonus,
                    dormant_bonus: dormant,
                    weather_penalty: c.weather_penalty,
                    final_score: c.style_score - c.recency_penalty + c.diversity_bonus + dormant
                        - c.weather_penalty,
                },
                candidate: (*c).clone(),
            }
        })
}

fn build_variation_scoring(c: &OutfitCandidate) -> ScoringDetail {
    let mode_score = c.style_score - (c.recency_penalty as f64 * 1.8).floor() as i32
        + (c.diversity_bonus as f64 * 2.5).floor() as i32
        - c.weather_penalty;
    ScoringDetail {
        style_score: c.style_score,
        recency_penalty: c.recency_penalty,
        diversity_bonus: c.diversity_bonus,
        dormant_bonus: 0,
        weather_penalty: c.weather_penalty,
        final_score: mode_score,
    }
}

pub async fn save_selected_recommendation(
    pool: &MySqlPool,
    user_id: &str,
    selected: &OutfitCandidate,
) -> Result<()> {
    RecommendationHistoryRepo::insert(
        pool,
        user_id,
        selected.top_id.as_deref(),
        selected.bottom_id.as_deref(),
        selected.outer_id.as_deref(),
        selected.shoes_id.as_deref(),
        selected.bag_id.as_deref(),
    )
    .await?;

    Ok(())
}
