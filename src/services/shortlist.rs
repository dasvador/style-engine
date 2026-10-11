//! Shortlist Generator — 150개 wardrobe에서 슬롯별 top-k를 추출.
//!
//! LLM에 전체 150개를 보내지 않고, situation/temperature/role/recency 기반으로
//! 각 슬롯별 shortlist만 만들어 전달한다.
//!
//! 다양성 확보를 위해 단순 점수 top-k가 아니라 role/style bucket을 섞는다.

use std::collections::{HashMap, HashSet};

use crate::models::clothing::Clothing;
use crate::models::style_vocab::{Role, Style, Weight};

/// 슬롯별 shortlist 크기
const TOP_K_TOP: usize = 12;
const TOP_K_BOTTOM: usize = 10;
const TOP_K_OUTER: usize = 8;
const TOP_K_SHOES: usize = 6;
const TOP_K_BAG: usize = 5;

/// Shortlist 생성 컨텍스트
// current_season 은 호출부가 채우지만 현재 shortlist 로직은 아직 참조하지 않는다.
#[allow(dead_code)]
pub struct ShortlistContext<'a> {
    pub temperature: f64,
    pub situation: Option<&'a str>,
    pub current_season: Option<&'a str>,
    pub recent_item_ids: &'a HashSet<String>,
}

/// 슬롯별 role 우선순위. 인덱스가 낮을수록 우선.
fn role_priority(category: &str, role: &str) -> i32 {
    match category {
        "상의" => match role {
            "베이스" => 0,
            "연결템" => 1,
            "구조템" => 2,
            "약한포인트" => 3,
            "포인트" => 4,
            _ => 5,
        },
        "하의" => match role {
            "베이스" => 0,
            "구조템" => 1,
            "연결템" => 2,
            "약한포인트" => 3,
            "포인트" => 4,
            _ => 5,
        },
        "아우터" => match role {
            "구조템" => 0,
            "연결템" => 1,
            "약한포인트" => 2,
            "포인트" => 3,
            "베이스" => 4,
            _ => 5,
        },
        "신발" | "가방" => match role {
            "연결템" => 0,
            "구조템" => 1,
            "베이스" => 2,
            "약한포인트" => 3,
            "포인트" => 4,
            _ => 5,
        },
        _ => 5,
    }
}

/// 아이템 하나의 shortlist 점수를 계산. 높을수록 shortlist에 포함될 가능성 높음.
fn score_item(item: &Clothing, ctx: &ShortlistContext) -> i32 {
    let mut s: i32 = 50; // base

    let role = item.role.map(|v| v.as_str()).unwrap_or("");
    let style = item.style.unwrap_or(Style::Basic);
    let formality = item.formality_level.unwrap_or(2) as i32;

    // 1. role 우선순위 보너스 (슬롯별 우선 role이 높은 점수)
    let rp = role_priority(&item.category, role);
    s += (5 - rp) * 3; // 0~15

    // 2. neutral core 보너스 — 베이직 스타일은 어디서나 살아남아야 함
    if style == Style::Basic {
        s += 5;
    }

    // 3. situation 기반 formality 적합성
    if let Some(sit) = ctx.situation {
        let (ideal_min, ideal_max) = match sit {
            "출근" | "비즈니스" => (3, 5),
            "데이트" => (2, 4),
            _ => (1, 3), // 캐주얼/일상
        };
        if formality >= ideal_min && formality <= ideal_max {
            s += 8;
        } else if formality < ideal_min {
            // under-formal: 상황 격식에 부족 → 감점 (비대칭: under가 더 강함)
            let gap = ideal_min - formality;
            s -= gap * 6;
        } else {
            // over-formal: 과한 격식 → 약한 감점
            let gap = formality - ideal_max;
            s -= gap * 2;
        }

        // 출근/데이트에서 스포츠 신발 강하게 하향
        if item.category == "신발" && style == Style::Sport {
            match sit {
                "출근" | "비즈니스" => s -= 20,
                "데이트" => s -= 12,
                _ => {}
            }
        }
    }

    // 4. recency penalty — 최근 추천된 아이템 소폭 하향
    if ctx.recent_item_ids.contains(&item.id) {
        s -= 5;
    }

    // 5. thematic (밀리터리/워크) 아이템은 neutral보다 약간 낮게
    // 단, 2개까지는 정상이므로 강하게 빼지 않음
    if style == Style::Military || style == Style::Work {
        s -= 2;
    }

    s
}

/// 온도 기반 아이템 필터 — 계절에 맞지 않는 아이템 제외
fn is_temp_appropriate(item: &Clothing, temp: f64) -> bool {
    let weight = item.weight.unwrap_or(Weight::Mid);
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    if temp >= 20.0 {
        if mat == "wool" || mat == "flannel" {
            return false;
        }
        if name.contains("니트") && !name.contains("가벼") {
            return false;
        }
        if name.contains("울 ") {
            return false;
        }
        if item.category == "아우터" && weight == Weight::Heavy {
            return false;
        }
        if name.contains("코트") || name.contains("파카") {
            return false;
        }
    }
    if temp >= 25.0 {
        if item.category == "아우터" && weight != Weight::Light {
            return false;
        }
        if name.contains("코듀로이") {
            return false;
        }
        if weight == Weight::Heavy {
            return false;
        }
    }
    true
}

/// 이 상황에서 입으면 오늘 입기 판정(`serving_ranker::compute_today_fit`)이 바로
/// "실패" 가 되는 옷인가.
///
/// 그 판정은 출근·비즈니스에서 스포츠 신발, 스웻 상하의, 평균 격식 2 미만을, 데이트에서
/// 스포츠 신발을 실패로 본다. 격식 1(후드티·스웻팬츠·반팔 티·러닝화 등)이 섞이면 평균이
/// 2 아래로 내려가기 쉬워서 출근에서는 격식 1 을 통째로 뺀다. 스웻 세트도 여기서 같이
/// 빠진다(스웻 옷은 전부 격식 1 이다).
///
/// 2026-10-11 측정: "여성 · 스트리트 · 출근" 요청에서 후보 5개가 모두 실패였다. 점수
/// 감점만으로는 옷이 적은 무드에서 이런 옷이 그대로 후보에 들어간다.
fn fails_situation(item: &Clothing, situation: Option<&str>) -> bool {
    let sport_shoes = item.category == "신발" && item.style == Some(Style::Sport);
    match situation {
        Some("출근" | "비즈니스") => sport_shoes || item.formality_level.unwrap_or(2) <= 1,
        Some("데이트") => sport_shoes,
        _ => false,
    }
}

/// 상황에 안 맞는 옷을 뺀 뒤 슬롯에 남는 옷이 이보다 적으면 빼지 않는다. 후보가 비면
/// 조합을 만들 수 없다 — 그때는 예전처럼 감점만 하고, 남은 실패는 추천 단계의
/// `unfit_gate` 가 거른다.
const MIN_AFTER_SITUATION_FILTER: usize = 2;

/// 무드로 걸러 낸 옷장에 이 상황에 맞는 옷이 슬롯당 두 벌보다 적으면, 같은 성별의
/// 전체 옷장에서 맞는 옷을 빌려 온다(2026-10-11 사용자 결정 — 무드보다 입을 수 있는
/// 코디가 먼저다).
///
/// 여성 미니멀은 격식 2 이상 신발이 한 켤레, 여성 스트리트는 상의·신발이 전부 격식 1
/// 이하라서 출근 추천의 후보 5개가 모두 실패였다. 빌리는 수는 모자란 만큼에 여유 하나를
/// 더한 정도로 묶는다 — 무드 옷장을 밀어내지 않기 위해서다.
///
/// 상황이 없거나 캐주얼이면 아무것도 빌리지 않는다. 빌린 옷은 격식이 상황 이상적 구간에
/// 가까운 순서로 고른다.
pub fn borrow_for_situation(
    mood_pool: &[Clothing],
    wardrobe: &[Clothing],
    ctx: &ShortlistContext,
) -> Vec<Clothing> {
    if !matches!(ctx.situation, Some("출근" | "비즈니스" | "데이트")) {
        return Vec::new();
    }
    let ok = |c: &Clothing| {
        is_temp_appropriate(c, ctx.temperature) && !fails_situation(c, ctx.situation)
    };
    let have: HashSet<&str> = mood_pool.iter().map(|c| c.id.as_str()).collect();
    let mut borrowed = Vec::new();
    for category in ["상의", "하의", "신발", "가방"] {
        let fitting = mood_pool
            .iter()
            .filter(|c| c.category == category && ok(c))
            .count();
        if fitting >= MIN_AFTER_SITUATION_FILTER {
            continue;
        }
        let want = MIN_AFTER_SITUATION_FILTER - fitting + 1;
        let mut extra: Vec<&Clothing> = wardrobe
            .iter()
            .filter(|c| c.category == category && !have.contains(c.id.as_str()) && ok(c))
            .collect();
        extra.sort_by_key(|c| std::cmp::Reverse(score_item(c, ctx)));
        borrowed.extend(extra.into_iter().take(want).cloned());
    }
    borrowed
}

/// 슬롯별 shortlist를 생성.
/// 단순 top-k가 아니라 role bucket을 섞어 다양성을 확보.
pub fn build_shortlist<'a>(
    clothes: &'a [Clothing],
    category: &str,
    k: usize,
    ctx: &ShortlistContext,
) -> Vec<&'a Clothing> {
    let in_slot: Vec<&Clothing> = clothes
        .iter()
        .filter(|c| c.category == category)
        .filter(|c| is_temp_appropriate(c, ctx.temperature))
        .collect();
    let fitting: Vec<&Clothing> = in_slot
        .iter()
        .copied()
        .filter(|c| !fails_situation(c, ctx.situation))
        .collect();
    let pool = if fitting.len() >= MIN_AFTER_SITUATION_FILTER {
        fitting
    } else {
        in_slot
    };
    let mut candidates: Vec<(&Clothing, i32)> =
        pool.into_iter().map(|c| (c, score_item(c, ctx))).collect();

    candidates.sort_by_key(|a| std::cmp::Reverse(a.1));

    if candidates.len() <= k {
        return candidates.into_iter().map(|(c, _)| c).collect();
    }

    // Bucket-diversified selection: role별로 최소 1개씩 보장한 뒤 나머지를 점수순으로 채움
    let mut selected: Vec<&Clothing> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    // Phase 1: 각 role에서 최고 점수 1개씩
    let mut by_role: HashMap<Role, Vec<(&Clothing, i32)>> = HashMap::new();
    for &(c, score) in &candidates {
        let role = c.role.unwrap_or(Role::Base);
        by_role.entry(role).or_default().push((c, score));
    }
    for items in by_role.values() {
        if let Some(&(best, _)) = items.first()
            && selected.len() < k
            && !used.contains(&best.id)
        {
            selected.push(best);
            used.insert(best.id.clone());
        }
    }

    // Phase 2: thematic bag 제한 — 가방에서 밀리터리/워크 style은 최대 1개
    let thematic_bag_limit = if category == "가방" { 1 } else { usize::MAX };
    let mut thematic_bag_count = selected
        .iter()
        .filter(|c| {
            c.category == "가방" && matches!(c.style, Some(Style::Military) | Some(Style::Work))
        })
        .count();

    // Phase 3: 나머지를 점수순으로 채움
    for &(c, _) in &candidates {
        if selected.len() >= k {
            break;
        }
        if used.contains(&c.id) {
            continue;
        }
        // thematic bag 제한 체크
        if category == "가방"
            && matches!(c.style, Some(Style::Military) | Some(Style::Work))
            && thematic_bag_count >= thematic_bag_limit
        {
            continue;
        }
        selected.push(c);
        used.insert(c.id.clone());
        if category == "가방" && matches!(c.style, Some(Style::Military) | Some(Style::Work)) {
            thematic_bag_count += 1;
        }
    }

    selected
}

/// 전체 wardrobe에서 모든 슬롯의 shortlist를 한 번에 생성.
pub fn build_all_shortlists<'a>(
    clothes: &'a [Clothing],
    ctx: &ShortlistContext,
) -> ShortlistResult<'a> {
    ShortlistResult {
        tops: build_shortlist(clothes, "상의", TOP_K_TOP, ctx),
        bottoms: build_shortlist(clothes, "하의", TOP_K_BOTTOM, ctx),
        outers: build_shortlist(clothes, "아우터", TOP_K_OUTER, ctx),
        shoes: build_shortlist(clothes, "신발", TOP_K_SHOES, ctx),
        bags: build_shortlist(clothes, "가방", TOP_K_BAG, ctx),
    }
}

pub struct ShortlistResult<'a> {
    pub tops: Vec<&'a Clothing>,
    pub bottoms: Vec<&'a Clothing>,
    pub outers: Vec<&'a Clothing>,
    pub shoes: Vec<&'a Clothing>,
    pub bags: Vec<&'a Clothing>,
}

impl<'a> ShortlistResult<'a> {
    /// GroupedClothes로 변환 (LLM 프롬프트용)
    pub fn to_grouped(&self) -> crate::services::prompts::GroupedClothes {
        let fmt = |c: &Clothing| {
            format!(
                "- {} | role:{} | tone:{} | style:{}",
                c.name,
                c.role.map(|v| v.as_str()).unwrap_or("-"),
                c.tone.map(|v| v.as_str()).unwrap_or("-"),
                c.style.map(|v| v.as_str()).unwrap_or("-"),
            )
        };
        crate::services::prompts::GroupedClothes {
            tops: self.tops.iter().map(|c| fmt(c)).collect(),
            bottoms: self.bottoms.iter().map(|c| fmt(c)).collect(),
            outers: self.outers.iter().map(|c| fmt(c)).collect(),
            shoes: self.shoes.iter().map(|c| fmt(c)).collect(),
            bags: self.bags.iter().map(|c| fmt(c)).collect(),
        }
    }

    /// 디버깅용 요약
    pub fn summary(&self) -> String {
        format!(
            "shortlist: tops={} bottoms={} outers={} shoes={} bags={}",
            self.tops.len(),
            self.bottoms.len(),
            self.outers.len(),
            self.shoes.len(),
            self.bags.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::style_vocab::Thickness;
    use chrono::NaiveDateTime;

    fn item(id: &str, category: &str, style: Style, formality: i8) -> Clothing {
        let ts = NaiveDateTime::parse_from_str("2026-10-11 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        Clothing {
            id: id.into(),
            name: id.into(),
            category: category.into(),
            gender: None,
            style_mood: None,
            color: None,
            thickness: Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: None,
            style: Some(style),
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: Some(formality),
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
            created_at: ts,
            updated_at: ts,
        }
    }

    fn ids(clothes: &[Clothing], category: &str, situation: Option<&str>) -> Vec<String> {
        let recent = HashSet::new();
        let ctx = ShortlistContext {
            temperature: 18.0,
            situation,
            current_season: None,
            recent_item_ids: &recent,
        };
        let mut v: Vec<String> = build_shortlist(clothes, category, 10, &ctx)
            .into_iter()
            .map(|c| c.id.clone())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn office_drops_sport_shoes_and_formality_one() {
        let clothes = vec![
            item("러닝화", "신발", Style::Sport, 1),
            item("로퍼", "신발", Style::Basic, 3),
            item("더비", "신발", Style::Formal, 4),
            item("후드티", "상의", Style::Basic, 1),
            item("셔츠", "상의", Style::Basic, 3),
            item("니트", "상의", Style::Basic, 2),
        ];
        assert_eq!(ids(&clothes, "신발", Some("출근")), vec!["더비", "로퍼"]);
        assert_eq!(ids(&clothes, "상의", Some("출근")), vec!["니트", "셔츠"]);
        // 데이트는 스포츠 신발만 뺀다. 캐주얼은 아무것도 빼지 않는다.
        assert_eq!(ids(&clothes, "신발", Some("데이트")), vec!["더비", "로퍼"]);
        assert_eq!(
            ids(&clothes, "상의", Some("데이트")),
            vec!["니트", "셔츠", "후드티"]
        );
        assert_eq!(
            ids(&clothes, "신발", Some("캐주얼")),
            vec!["더비", "러닝화", "로퍼"]
        );
    }

    /// 무드 옷장에 출근용 신발이 모자라면 전체 옷장에서 빌려 온다. 충분하면 빌리지 않는다.
    #[test]
    fn office_borrows_only_for_short_slots() {
        let mood = vec![
            item("러닝화", "신발", Style::Sport, 1),
            item("캔버스", "신발", Style::Basic, 1),
            item("셔츠", "상의", Style::Basic, 3),
            item("니트", "상의", Style::Basic, 2),
        ];
        let mut wardrobe = mood.clone();
        wardrobe.push(item("로퍼", "신발", Style::Basic, 3));
        wardrobe.push(item("더비", "신발", Style::Formal, 4));
        wardrobe.push(item("블라우스", "상의", Style::Basic, 3));
        let recent = HashSet::new();
        let ctx = |sit| ShortlistContext {
            temperature: 18.0,
            situation: sit,
            current_season: None,
            recent_item_ids: &recent,
        };

        let mut got: Vec<String> = borrow_for_situation(&mood, &wardrobe, &ctx(Some("출근")))
            .into_iter()
            .map(|c| c.id)
            .collect();
        got.sort();
        assert_eq!(got, vec!["더비", "로퍼"]); // 신발만, 상의는 이미 두 벌
        assert!(borrow_for_situation(&mood, &wardrobe, &ctx(Some("캐주얼"))).is_empty());
    }

    /// 빼고 나서 두 벌이 안 남으면 빼지 않는다 — 빈 후보로는 조합을 못 만든다.
    #[test]
    fn too_few_left_keeps_the_slot_as_it_was() {
        let clothes = vec![
            item("후드티", "상의", Style::Basic, 1),
            item("그래픽 티", "상의", Style::Basic, 1),
            item("셔츠", "상의", Style::Basic, 3),
        ];
        assert_eq!(
            ids(&clothes, "상의", Some("출근")),
            vec!["그래픽 티", "셔츠", "후드티"]
        );
    }
}
