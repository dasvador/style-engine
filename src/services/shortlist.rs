//! Shortlist Generator — 150개 wardrobe에서 슬롯별 top-k를 추출.
//!
//! LLM에 전체 150개를 보내지 않고, situation/temperature/role/recency 기반으로
//! 각 슬롯별 shortlist만 만들어 전달한다.
//!
//! 다양성 확보를 위해 단순 점수 top-k가 아니라 role/style bucket을 섞는다.

use std::collections::{HashMap, HashSet};

use crate::models::clothing::Clothing;
use crate::models::outfit::SlotKind;
use crate::models::style_vocab::{Role, Style, StyleGenre, Weight};
use crate::services::item_resolution::slot_ref;

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
/// 신발이 이 기온에 맞는가. 발을 얼마나 덮는지로 나눈다 — 두께 값은 신발을 가르지
/// 못한다(옷장의 신발은 거의 다 medium 이다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShoeWeather {
    Fine,
    /// 맞지는 않지만 입을 수는 있다. 순위에서 조금 밀 뿐 빼지는 않는다.
    Borderline,
    /// 확실히 맞지 않는다. 후보에서 뺀다.
    Unsuitable,
}

/// 2026-10-17: 27°C 에 니하이 롱부츠와 앵클부츠가 오늘의 추천에 올랐다. 신발은 보온
/// 판정(상의+아우터)에 들어가지 않아 기온을 전혀 보지 않았다. "27°C 면 부츠 금지" 같은
/// 한 가지 규칙으로 막지 않고, 덮는 범위와 기온을 나눈다.
pub fn shoe_weather(item: &Clothing, temp: f64) -> ShoeWeather {
    if item.category != "신발" {
        return ShoeWeather::Fine;
    }
    let sub = item.sub_category.as_deref().unwrap_or("");
    let name = item.name.as_str();
    let knee_high = sub == "롱부츠" || name.contains("롱부츠") || name.contains("니하이");
    let boot = knee_high || sub.contains("부츠") || sub.contains("boots") || name.contains("부츠");
    let open = sub.contains("샌들")
        || sub == "뮬"
        || name.contains("샌들")
        || name.contains("뮬")
        || name.contains("슬라이드");
    if knee_high && temp >= 25.0 {
        return ShoeWeather::Unsuitable;
    }
    if boot && temp >= 27.0 {
        return ShoeWeather::Borderline;
    }
    if open && temp < 8.0 {
        return ShoeWeather::Unsuitable;
    }
    if open && temp < 13.0 {
        return ShoeWeather::Borderline;
    }
    ShoeWeather::Fine
}

fn is_temp_appropriate(item: &Clothing, temp: f64) -> bool {
    if shoe_weather(item, temp) == ShoeWeather::Unsuitable {
        return false;
    }
    let weight = item.weight.unwrap_or(Weight::Mid);
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    if temp >= 20.0 {
        // 벨벳은 울처럼 보온·무게가 있는 원단이다. 글램 시드에서 처음 들어왔고, 빠져 있으면
        // 27°C 에 벨벳 탑이 후보에 오른다.
        if mat == "wool" || mat == "flannel" || mat == "velvet" {
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
        // 가죽 상의·하의는 바람이 통하지 않는다. 엣지 시드의 레더 미디 스커트가 26°C 후보에
        // 올랐다(2026-10-17). 가죽 신발·가방은 그대로 둔다. 소재가 저장되지 않는 업로드 옷도
        // 이름으로 가린다(`mood_gate::is_leather_garment`).
        if item.category != "아우터" && crate::services::mood_gate::is_leather_garment(item) {
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
/// 상황이 없거나 캐주얼이면 아무것도 빌리지 않는다.
///
/// 빌릴 옷은 무드와 가까운 장르의 옷부터 고른다(`mood_affinity`). 격식만 보고 골랐을 때
/// 미니멀 출근에 에스닉 자수 블라우스가 들어갔다 — 격식은 맞지만 무드와 멀다. 같은
/// 가까움이면 상황 점수(`score_item`) 순서다.
pub fn borrow_for_situation(
    mood_pool: &[Clothing],
    wardrobe: &[Clothing],
    ctx: &ShortlistContext,
    mood: Option<StyleGenre>,
    genres: &HashMap<String, Vec<String>>,
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
        extra.sort_by_key(|c| {
            let affinity = mood.map_or(0, |m| mood_affinity(m, genres.get(&c.id)));
            std::cmp::Reverse((affinity, score_item(c, ctx)))
        });
        borrowed.extend(extra.into_iter().take(want).cloned());
    }
    borrowed
}

/// 무드에서 가까운 장르. 앞에 있을수록 가깝다. 무드 밖에서 옷을 빌릴 때 이 순서로 고른다.
///
/// 장르 설명(`StyleGenre`)과 옷장의 장르 겹침에서 정했다 — 예를 들어 미니멀과 클래식은
/// 셔츠·트렌치를 공유하고, 스트리트와 스포티 캐주얼은 후디·트랙 재킷을 공유한다.
fn mood_neighbors(mood: StyleGenre) -> &'static [StyleGenre] {
    use StyleGenre::*;
    match mood {
        Minimal => &[Classic, ModernChic, Mannish],
        Classic => &[Minimal, Preppy, Mannish, ModernChic],
        ModernChic => &[Minimal, Classic, Mannish],
        Mannish => &[Classic, ModernChic, Minimal],
        Street => &[SportyCasual, Amekaji, Minimal],
        SportyCasual => &[Street, OutdoorCasual, Minimal],
        Amekaji => &[Workwear, Preppy, Street, Classic],
        Preppy => &[Classic, Amekaji, Minimal],
        Workwear => &[Amekaji, OutdoorCasual],
        OutdoorCasual => &[Workwear, SportyCasual],
        Romantic => &[Classic, Bohemian, Minimal],
        Bohemian => &[Romantic, Amekaji],
        // 글램의 포인트를 받쳐 주는 쪽은 날렵한 모던 시크와 기본 아이템인 미니멀이다.
        // 새틴 같은 소재를 공유하는 로맨틱은 그다음이다 — 먼저 빌리면 부드러운 인상이 섞인다.
        Glam => &[ModernChic, Minimal, Romantic],
        // 엣지는 구조적인 재단을 모던 시크·매니시에서, 데님을 스트리트에서 빌린다. 기본
        // 아이템인 미니멀은 마지막이다 — 먼저 빌리면 형태가 없는 코디가 늘어난다.
        Edgy => &[ModernChic, Mannish, Street, Minimal],
        // 팝은 색 있는 옷을 스트리트·스포티 캐주얼에서, 부드러운 색을 로맨틱에서 빌린다. 받쳐 줄
        // 기본 아이템은 미니멀이 마지막이다.
        Pop => &[Street, SportyCasual, Romantic, Minimal],
    }
}

/// 옷의 장르가 무드에 얼마나 가까운가. 가까운 이웃 장르를 가질수록 크다. 무드에서 먼
/// 장르(이웃 목록에 없는 것)만 가진 옷은 0 이다.
fn mood_affinity(mood: StyleGenre, item_genres: Option<&Vec<String>>) -> i32 {
    let neighbors = mood_neighbors(mood);
    let Some(gs) = item_genres else { return 0 };
    gs.iter()
        .filter_map(|g| g.parse::<StyleGenre>().ok())
        .filter_map(|g| neighbors.iter().position(|n| *n == g))
        .map(|rank| (neighbors.len() - rank) as i32)
        .sum()
}

/// 슬롯별 shortlist를 생성.
/// 단순 top-k가 아니라 role bucket을 섞어 다양성을 확보.
/// 이 슬롯에서 날씨·상황을 지난 옷과 점수. 점수 높은 순이다.
fn eligible<'a>(
    clothes: &'a [Clothing],
    category: &str,
    ctx: &ShortlistContext,
) -> Vec<(&'a Clothing, i32)> {
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
    candidates
}

pub fn build_shortlist<'a>(
    clothes: &'a [Clothing],
    category: &str,
    k: usize,
    ctx: &ShortlistContext,
) -> Vec<&'a Clothing> {
    let candidates = eligible(clothes, category, ctx);

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

/// 후보 목록에 새로 넣을 수 있는 색 계열의 수(슬롯당). 기본색 옷이 밀려나지 않게 적게 둔다.
const MAX_ADDED_COLOUR_FAMILIES: usize = 3;

/// 색 다양성을 다루는 성별·무드(`mood_palette::targets`)의 후보 목록. 기본 목록을 그대로 만든
/// 뒤, 상의·아우터에서 목록에 없는 색 계열을 계열마다 가장 점수 높은 옷 한 벌씩(슬롯당 최대
/// `MAX_ADDED_COLOUR_FAMILIES`) 더한다.
///
/// 2026-10-17 남성 아메카지(감사 `mood-color-audit-20261017`): 옷장의 컬러 상의·아우터 9벌 중
/// 19.6°C 첫 후보 목록에 4벌만 들어갔고, 브릭레드 플란넬·로얄블루 워크 셔츠는 빠졌다. 이 목록은
/// 역할·베이직 스타일 우선 점수로 고르고 색은 보지 않는다.
///
/// 날씨·상황을 지난 옷(`eligible`)에서만 고른다. 목록이 차 있으면 점수가 가장 낮은 옷과
/// 바꾸되, 그 역할의 유일한 옷과 그 색 계열의 유일한 옷은 빼지 않는다. 특정 옷을 지목하지
/// 않는다. 색 있는 옷이 없으면 기본 목록 그대로다.
pub fn build_all_shortlists_with_palette<'a>(
    clothes: &'a [Clothing],
    ctx: &ShortlistContext,
) -> ShortlistResult<'a> {
    let base = build_all_shortlists(clothes, ctx);
    // 모델에 보내는 목록에서 깨진 이름의 옷을 빼고 다음 점수의 정상 옷으로 채운 뒤 색 계열을
    // 보강한다. 이 순서라 깨진 옷이 색 계열 자리를 막지도 않는다.
    let slot = |selected: Vec<&'a Clothing>, category: &str, k: usize| {
        without_broken_names(selected, &eligible(clothes, category, ctx), k)
    };
    let tops = slot(base.tops, "상의", TOP_K_TOP);
    let outers = slot(base.outers, "아우터", TOP_K_OUTER);
    ShortlistResult {
        tops: add_colour_families(tops, &eligible(clothes, "상의", ctx), TOP_K_TOP),
        outers: add_colour_families(outers, &eligible(clothes, "아우터", ctx), TOP_K_OUTER),
        bottoms: slot(base.bottoms, "하의", TOP_K_BOTTOM),
        shoes: slot(base.shoes, "신발", TOP_K_SHOES),
        bags: slot(base.bags, "가방", TOP_K_BAG),
    }
}

/// 이름이 깨진 옷을 빼고, 빈 자리를 같은 슬롯의 다음 점수 정상 옷으로 채운다(색 다양성 대상만).
///
/// 2026-10-17 프레피·아메카지 실측: 이름에 깨진 글자가 있는 "슬레이트블루 럭비 셔츠" 를 모델이
/// 고쳐 써서 돌려주고, 서버는 고친 이름을 옷장에서 찾지 못해 그 초안을 버렸다 — 색 목록에서만
/// 빼도 일반 상의 목록에 남아 계속 골랐다. DB 원본과 다른 무드의 목록은 그대로다.
fn without_broken_names<'a>(
    selected: Vec<&'a Clothing>,
    eligible: &[(&'a Clothing, i32)],
    k: usize,
) -> Vec<&'a Clothing> {
    let mut kept: Vec<&Clothing> = selected
        .into_iter()
        .filter(|c| !has_broken_name(c))
        .collect();
    for (c, _) in eligible {
        if kept.len() >= k {
            break;
        }
        if !has_broken_name(c) && !kept.iter().any(|x| x.id == c.id) {
            kept.push(c);
        }
    }
    kept
}

/// 이름에 깨진 글자(U+FFFD)가 있는가. 모델이 그대로 돌려줄 수 없는 이름이다.
fn has_broken_name(c: &Clothing) -> bool {
    c.name.contains('\u{FFFD}')
}

fn add_colour_families<'a>(
    mut selected: Vec<&'a Clothing>,
    eligible: &[(&'a Clothing, i32)],
    k: usize,
) -> Vec<&'a Clothing> {
    use crate::services::mood_palette::colour_family;
    let score_of = |c: &Clothing| {
        eligible
            .iter()
            .find(|(e, _)| e.id == c.id)
            .map_or(i32::MIN, |(_, s)| *s)
    };
    let mut added = 0;
    for (c, _) in eligible {
        if added >= MAX_ADDED_COLOUR_FAMILIES {
            break;
        }
        // 이름이 깨진 옷은 색 계열 대표로 고르지 않는다(`colour_items` 참고).
        if has_broken_name(c) {
            continue;
        }
        let Some(family) = colour_family(c) else {
            continue;
        };
        // 대표는 모델이 고를 수 있는 옷만 친다 — 깨진 이름의 같은 계열 옷은 대표가 아니다.
        if selected
            .iter()
            .any(|x| !has_broken_name(x) && colour_family(x) == Some(family))
        {
            continue;
        }
        if selected.len() >= k {
            // 빼도 되는 옷: 같은 역할이 하나 더 있고, 색 계열이 없거나 같은 계열이 하나 더 있다.
            let victim = selected
                .iter()
                .enumerate()
                .filter(|(_, x)| {
                    selected.iter().filter(|y| y.role == x.role).count() >= 2
                        && colour_family(x).is_none_or(|f| {
                            selected
                                .iter()
                                .filter(|y| colour_family(y) == Some(f))
                                .count()
                                >= 2
                        })
                })
                .min_by_key(|(_, x)| score_of(x))
                .map(|(i, _)| i);
            let Some(i) = victim else { break };
            selected.remove(i);
        }
        selected.push(c);
        added += 1;
    }
    selected
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
        let fmt = |c: &Clothing, r: String| {
            format!(
                "- {} | id:{} | role:{} | tone:{} | style:{}",
                c.name,
                r,
                c.role.map(|v| v.as_str()).unwrap_or("-"),
                c.tone.map(|v| v.as_str()).unwrap_or("-"),
                c.style.map(|v| v.as_str()).unwrap_or("-"),
            )
        };
        crate::services::prompts::GroupedClothes {
            tops: self
                .tops
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Top, i)))
                .collect(),
            bottoms: self
                .bottoms
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Bottom, i)))
                .collect(),
            outers: self
                .outers
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Outer, i)))
                .collect(),
            shoes: self
                .shoes
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Shoes, i)))
                .collect(),
            bags: self
                .bags
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Bag, i)))
                .collect(),
            colour_items: Vec::new(),
            texture_pair: None,
        }
    }

    /// 글램 후보 생성용. 이름·역할·톤·스타일에 소재·실루엣·두께·특징을 더한다. 글램은
    /// 어떤 옷이 빛을 받는지, 어떤 형태인지가 판단의 중심인데 이름만으로는 모자라다.
    /// 값이 없으면 "모름" 이라고 적는다 — 채워 넣지 않는다.
    pub fn to_grouped_detailed(&self) -> crate::services::prompts::GroupedClothes {
        let fmt = |c: &Clothing, r: String| {
            let known = |v: Option<&str>| match v.map(str::trim) {
                Some(v) if !v.is_empty() => v.to_string(),
                _ => "모름".to_string(),
            };
            format!(
                "- {} | id:{} | role:{} | tone:{} | style:{} | 소재:{} | 실루엣:{} | 두께:{} | 특징:{}",
                c.name,
                r,
                c.role.map(|v| v.as_str()).unwrap_or("모름"),
                c.tone.map(|v| v.as_str()).unwrap_or("모름"),
                c.style.map(|v| v.as_str()).unwrap_or("모름"),
                known(c.material_primary.as_deref()),
                c.silhouette_volume.map(|v| v.as_str()).unwrap_or("모름"),
                c.thickness.as_str(),
                known(c.texture_keywords.as_deref()),
            )
        };
        crate::services::prompts::GroupedClothes {
            tops: self
                .tops
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Top, i)))
                .collect(),
            bottoms: self
                .bottoms
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Bottom, i)))
                .collect(),
            outers: self
                .outers
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Outer, i)))
                .collect(),
            shoes: self
                .shoes
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Shoes, i)))
                .collect(),
            bags: self
                .bags
                .iter()
                .enumerate()
                .map(|(i, c)| fmt(c, slot_ref(SlotKind::Bag, i)))
                .collect(),
            colour_items: Vec::new(),
            texture_pair: None,
        }
    }

    /// 모델에게 보여 준 슬롯별 목록과 같은 순서·참조의 연결 범위.
    pub fn offered_items(&self) -> crate::services::item_resolution::OfferedItems<'a> {
        crate::services::item_resolution::OfferedItems::from_slot_lists(
            &self.tops,
            &self.bottoms,
            &self.outers,
            &self.shoes,
            &self.bags,
        )
    }

    /// 글램 소재 대비 짝의 재료: 목록의 퍼·시어링 아우터와 무광 트리밍 상의. 둘 중 하나라도
    /// 없으면 `None`. 깨진 이름은 넣지 않는다.
    pub fn glam_texture_pair(&self) -> Option<crate::services::prompts::TexturePair> {
        use crate::services::mood_rules::{is_fur_or_shearling_outer, is_trimmed_matte_top};
        let names = |v: &[&Clothing], f: fn(&Clothing) -> bool| -> Vec<String> {
            v.iter()
                .filter(|c| !has_broken_name(c) && f(c))
                .map(|c| c.name.clone())
                .collect()
        };
        let outers = names(&self.outers, is_fur_or_shearling_outer);
        let tops = names(&self.tops, is_trimmed_matte_top);
        (!outers.is_empty() && !tops.is_empty())
            .then_some(crate::services::prompts::TexturePair { outers, tops })
    }

    /// 목록의 색 있는 상의·아우터(`mood_palette::colour_family`), "이름(계열)" 꼴.
    ///
    /// 이름에 깨진 글자(U+FFFD)가 있는 옷은 넣지 않는다. 모델은 그 이름을 그대로 돌려줄 수 없어
    /// 고쳐 쓰고, 서버는 고친 이름을 옷장에서 찾지 못해 그 초안을 버린다 — 2026-10-17 프레피
    /// 실측에서 "슬레이트블루 럭비 셔츠" 초안이 매번 이렇게 빠졌다. 데이터는 고치지 않는다.
    pub fn colour_items(&self) -> Vec<String> {
        self.tops
            .iter()
            .chain(self.outers.iter())
            .filter(|c| !has_broken_name(c))
            .filter_map(|c| {
                crate::services::mood_palette::colour_family(c).map(|f| format!("{}({f})", c.name))
            })
            .collect()
    }

    /// 팝 후보 지시에 넣을 팝 옷(상의·하의·아우터): 색 있는 배색·그래픽 옷은 "(배색·그래픽)",
    /// 선명한 색 옷은 "(색 계열)". 깨진 이름은 넣지 않는다(`colour_items` 참고).
    pub fn pop_items(&self) -> Vec<String> {
        use crate::services::mood_gate::{is_pop_piece, is_vivid_colour};
        self.tops
            .iter()
            .chain(self.bottoms.iter())
            .chain(self.outers.iter())
            .filter(|c| !has_broken_name(c))
            .filter_map(|c| {
                if is_pop_piece(c) {
                    Some(format!("{}(배색·그래픽)", c.name))
                } else if is_vivid_colour(c) {
                    crate::services::mood_palette::colour_family(c)
                        .map(|f| format!("{}({f})", c.name))
                } else {
                    None
                }
            })
            .collect()
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

    /// 더운 날에는 가죽 상의·하의를 빼고, 가죽 샌들·가방은 남긴다. 선선한 날에는 가죽
    /// 스커트도 후보다.
    #[test]
    fn leather_garments_leave_on_hot_days_but_leather_accessories_stay() {
        let leather = |id: &str, cat: &str| {
            let mut c = item(id, cat, Style::Basic, 3);
            c.material_primary = Some("leather".into());
            c
        };
        let skirt = leather("레더 미디 스커트", "하의");
        let sandal = leather("피셔맨 샌들", "신발");
        let bag = leather("숄더백", "가방");
        assert!(!is_temp_appropriate(&skirt, 26.0));
        assert!(is_temp_appropriate(&skirt, 18.0));
        assert!(is_temp_appropriate(&sandal, 26.0));
        assert!(is_temp_appropriate(&bag, 26.0));
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

        let none = HashMap::new();
        let mut got: Vec<String> =
            borrow_for_situation(&mood, &wardrobe, &ctx(Some("출근")), None, &none)
                .into_iter()
                .map(|c| c.id)
                .collect();
        got.sort();
        assert_eq!(got, vec!["더비", "로퍼"]); // 신발만, 상의는 이미 두 벌
        assert!(
            borrow_for_situation(&mood, &wardrobe, &ctx(Some("캐주얼")), None, &none).is_empty()
        );
    }

    /// 미니멀 출근에 상의가 모자라면, 격식이 더 높아도 무드에서 먼 자수 블라우스보다
    /// 클래식 셔츠를 먼저 빌린다.
    #[test]
    fn borrowing_prefers_garments_near_the_mood() {
        let mood = vec![item("크롭 티", "상의", Style::Basic, 1)];
        let mut wardrobe = mood.clone();
        wardrobe.push(item("자수 블라우스", "상의", Style::Basic, 3));
        wardrobe.push(item("옥스퍼드 셔츠", "상의", Style::Basic, 2));
        wardrobe.push(item("코튼 블라우스", "상의", Style::Basic, 3));
        let genres: HashMap<String, Vec<String>> = [
            ("자수 블라우스", vec!["romantic", "bohemian"]),
            ("옥스퍼드 셔츠", vec!["classic", "preppy"]),
            ("코튼 블라우스", vec!["romantic", "classic"]),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
        .collect();
        let recent = HashSet::new();
        let ctx = ShortlistContext {
            temperature: 18.0,
            situation: Some("출근"),
            current_season: None,
            recent_item_ids: &recent,
        };

        let got: Vec<String> =
            borrow_for_situation(&mood, &wardrobe, &ctx, Some(StyleGenre::Minimal), &genres)
                .into_iter()
                .map(|c| c.id)
                .collect();
        // 두 벌 모자라 세 벌까지 빌린다. 자수 블라우스는 장르가 미니멀과 멀어 맨 뒤다
        // (격식은 셋 중 가장 높아도). 앞의 둘은 둘 다 클래식이라 격식 점수 순서다.
        assert_eq!(got.len(), 3);
        assert_eq!(got[2], "자수 블라우스");
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
