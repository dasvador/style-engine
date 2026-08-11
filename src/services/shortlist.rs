//! Shortlist Generator — 150개 wardrobe에서 슬롯별 top-k를 추출.
//!
//! LLM에 전체 150개를 보내지 않고, situation/temperature/role/recency 기반으로
//! 각 슬롯별 shortlist만 만들어 전달한다.
//!
//! 다양성 확보를 위해 단순 점수 top-k가 아니라 role/style bucket을 섞는다.

use std::collections::{HashMap, HashSet};

use crate::models::clothing::Clothing;

/// 슬롯별 shortlist 크기
const TOP_K_TOP: usize = 12;
const TOP_K_BOTTOM: usize = 10;
const TOP_K_OUTER: usize = 8;
const TOP_K_SHOES: usize = 6;
const TOP_K_BAG: usize = 5;

/// Shortlist 생성 컨텍스트
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

    let role = item.role.as_deref().unwrap_or("");
    let style = item.style.as_deref().unwrap_or("베이직");
    let formality = item.formality_level.unwrap_or(2) as i32;

    // 1. role 우선순위 보너스 (슬롯별 우선 role이 높은 점수)
    let rp = role_priority(&item.category, role);
    s += (5 - rp) * 3; // 0~15

    // 2. neutral core 보너스 — 베이직 스타일은 어디서나 살아남아야 함
    if style == "베이직" {
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
        if item.category == "신발" && style == "스포츠" {
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
    if style == "밀리터리" || style == "워크" {
        s -= 2;
    }

    s
}

/// 온도 기반 아이템 필터 — 계절에 맞지 않는 아이템 제외
fn is_temp_appropriate(item: &Clothing, temp: f64) -> bool {
    let weight = item.weight.as_deref().unwrap_or("중간");
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    if temp >= 20.0 {
        if mat == "wool" || mat == "flannel" { return false; }
        if name.contains("니트") && !name.contains("가벼") { return false; }
        if name.contains("울 ") { return false; }
        if item.category == "아우터" && weight == "무거움" { return false; }
        if name.contains("코트") || name.contains("파카") { return false; }
    }
    if temp >= 25.0 {
        if item.category == "아우터" && weight != "가벼움" { return false; }
        if name.contains("코듀로이") { return false; }
        if weight == "무거움" { return false; }
    }
    true
}

/// 슬롯별 shortlist를 생성.
/// 단순 top-k가 아니라 role bucket을 섞어 다양성을 확보.
pub fn build_shortlist<'a>(
    clothes: &'a [Clothing],
    category: &str,
    k: usize,
    ctx: &ShortlistContext,
) -> Vec<&'a Clothing> {
    let mut candidates: Vec<(&Clothing, i32)> = clothes
        .iter()
        .filter(|c| c.category == category)
        .filter(|c| is_temp_appropriate(c, ctx.temperature))
        .map(|c| (c, score_item(c, ctx)))
        .collect();

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    if candidates.len() <= k {
        return candidates.into_iter().map(|(c, _)| c).collect();
    }

    // Bucket-diversified selection: role별로 최소 1개씩 보장한 뒤 나머지를 점수순으로 채움
    let mut selected: Vec<&Clothing> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    // Phase 1: 각 role에서 최고 점수 1개씩
    let mut by_role: HashMap<&str, Vec<(&Clothing, i32)>> = HashMap::new();
    for &(c, score) in &candidates {
        let role = c.role.as_deref().unwrap_or("베이스");
        by_role.entry(role).or_default().push((c, score));
    }
    for (_role, items) in &by_role {
        if let Some(&(best, _)) = items.first() {
            if selected.len() < k && !used.contains(&best.id) {
                selected.push(best);
                used.insert(best.id.clone());
            }
        }
    }

    // Phase 2: thematic bag 제한 — 가방에서 밀리터리/워크 style은 최대 1개
    let thematic_bag_limit = if category == "가방" { 1 } else { usize::MAX };
    let mut thematic_bag_count = selected
        .iter()
        .filter(|c| {
            c.category == "가방"
                && matches!(c.style.as_deref(), Some("밀리터리") | Some("워크"))
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
            && matches!(c.style.as_deref(), Some("밀리터리") | Some("워크"))
            && thematic_bag_count >= thematic_bag_limit
        {
            continue;
        }
        selected.push(c);
        used.insert(c.id.clone());
        if category == "가방"
            && matches!(c.style.as_deref(), Some("밀리터리") | Some("워크"))
        {
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
    pub fn to_grouped(&self) -> crate::services::openai::GroupedClothes {
        let fmt = |c: &Clothing| {
            format!(
                "- {} | role:{} | tone:{} | style:{}",
                c.name,
                c.role.as_deref().unwrap_or("-"),
                c.tone.as_deref().unwrap_or("-"),
                c.style.as_deref().unwrap_or("-"),
            )
        };
        crate::services::openai::GroupedClothes {
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
