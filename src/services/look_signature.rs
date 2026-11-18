//! 룩 시그니처 — 연속 추천의 "같아 보임" 을 옷 id 가 아니라 보이는 배색과 하의 형태로 비교한다
//! (2026-10-18, 엣지 다양성).
//!
//! 이전 다양성 장치는 id 로만 비교했다: 반복 감점(`recommendation_diversity`)은 같은 옷,
//! `cross_mood::drop_repeats` 는 id 가 하나 이하로 다른 코디를 본다. 그래서 블랙 탱크 + 차콜
//! 와이드 슬랙스 다음에 블랙 니트 + 차콜 배럴 팬츠가 와도 "다른 코디" 였다. 사용자 눈에는 같은
//! 룩이다(2026-10-18 사용자 보고). 이 모듈은 비교에 쓸 최소한의 신호만 정한다 — 무엇을 할지는
//! 호출하는 쪽이 정한다.
//!
//! - 보이는 색: 상의·아우터의 색 계열(`mood_palette::colour_family`, 채도 낮음 포함). 12°C 아래에서
//!   아우터가 있으면 여민 코트 속 상의 색은 보이지 않는다고 본다. 색 계열이 없으면 무채색이다.
//! - 하의 형태: 저장된 하위 분류·설명·이름으로 스커트·배럴·플레어·슬림·와이드·스트레이트로 나눈다.
//!   모르면 `Other` 다(같은 룩이라고 단정하지 않는다).
//! - 하의 톤: 어두운 하의(블랙·차콜·네이비 등 어두움 톤)인가.

use crate::models::clothing::Clothing;

/// 하의의 형태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BottomShape {
    Skirt,
    Barrel,
    Flare,
    Slim,
    Wide,
    Straight,
    Other,
}

impl BottomShape {
    pub fn as_str(self) -> &'static str {
        match self {
            BottomShape::Skirt => "스커트",
            BottomShape::Barrel => "배럴",
            BottomShape::Flare => "플레어",
            BottomShape::Slim => "슬림",
            BottomShape::Wide => "와이드",
            BottomShape::Straight => "스트레이트",
            BottomShape::Other => "기타",
        }
    }
}

/// 하의 형태. 스커트 → 배럴 → 플레어 → 슬림·테이퍼드 → 와이드 → 스트레이트 순으로 본다 —
/// "wide straight leg" 는 와이드다.
pub fn bottom_shape(c: &Clothing) -> BottomShape {
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let sub = c.sub_category.as_deref().unwrap_or("");
    let n = c.name.as_str();
    let has = |ws: &[&str]| {
        ws.iter()
            .any(|w| k.contains(w) || n.contains(w) || sub.contains(w))
    };
    if has(&["스커트", "skirt"]) {
        BottomShape::Skirt
    } else if has(&["barrel", "배럴", "balloon", "벌룬"]) {
        BottomShape::Barrel
    } else if has(&["flare", "플레어", "bootcut", "부츠컷"]) {
        BottomShape::Flare
    } else if has(&["slim", "skinny", "tapered", "슬림", "스키니", "테이퍼드"]) {
        BottomShape::Slim
    } else if has(&["wide", "와이드"]) {
        BottomShape::Wide
    } else if has(&["straight", "스트레이트"]) {
        BottomShape::Straight
    } else {
        BottomShape::Other
    }
}

/// 어두운 하의인가 — 톤이 어두움이거나, 톤을 모르면 이름의 기본 어두운 색.
pub fn is_dark(c: &Clothing) -> bool {
    match c.tone {
        Some(t) => t == crate::models::style_vocab::Tone::Dark,
        None => ["블랙", "차콜", "네이비", "다크"]
            .iter()
            .any(|w| c.color.as_deref().unwrap_or(&c.name).contains(w)),
    }
}

/// 한 코디의 시그니처.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookSignature {
    /// 보이는 상의·아우터의 색 계열(정렬). 비어 있으면 무채색 룩이다.
    pub colours: Vec<&'static str>,
    pub bottom_shape: BottomShape,
    pub bottom_dark: bool,
}

impl LookSignature {
    /// 보이는 색이 하나도 없는 룩.
    pub fn achromatic(&self) -> bool {
        self.colours.is_empty()
    }

    /// 사람 눈에 같은 룩인가 — 보이는 배색과 하의 형태·톤이 같다. 하의 형태를 모르면 같다고
    /// 단정하지 않는다.
    pub fn looks_like(&self, other: &LookSignature) -> bool {
        self.bottom_shape != BottomShape::Other
            && self.colours == other.colours
            && self.bottom_shape == other.bottom_shape
            && self.bottom_dark == other.bottom_dark
    }

    /// 기록용 짧은 표기.
    pub fn label(&self) -> String {
        format!(
            "{} / {}{}",
            if self.colours.is_empty() {
                "무채색".to_string()
            } else {
                self.colours.join("+")
            },
            if self.bottom_dark { "어두운 " } else { "" },
            self.bottom_shape.as_str()
        )
    }
}

/// 상의·하의·아우터로 시그니처를 만든다. 하의가 없으면 `None`.
pub fn signature(
    top: Option<&Clothing>,
    bottom: Option<&Clothing>,
    outer: Option<&Clothing>,
    temperature: Option<f64>,
) -> Option<LookSignature> {
    let bottom = bottom?;
    let closed = outer.is_some()
        && temperature.is_some_and(|t| t < crate::services::serving_ranker::CLOSED_COAT_BELOW_C);
    let mut colours: Vec<&'static str> = [if closed { None } else { top }, outer]
        .into_iter()
        .flatten()
        .filter_map(crate::services::mood_palette::colour_family)
        .collect();
    colours.sort();
    colours.dedup();
    Some(LookSignature {
        colours,
        bottom_shape: bottom_shape(bottom),
        bottom_dark: is_dark(bottom),
    })
}

/// 외투가 사람 눈에 어떻게 보이는가 — 네 축(2026-11-18 엣지: "추운 날 카드 외투가 늘 차콜·블랙의
/// 같은 울 롱코트"). 보온 충전재·안감이 아니라 **눈에 보이는 겉감**을 본다: 다운 퍼퍼의 겉은
/// 나일론이고, 가죽 시어링 재킷의 겉은 가죽이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OuterLook {
    pub id: String,
    pub name: String,
    /// 색 계열(`mood_palette::colour_family`), 없으면 뉴트럴 묶음(어두운 무채색·브라운·…).
    pub colour: &'static str,
    /// 눈에 보이는 겉감. 모르면 `None`(같다고 단정하지 않는다).
    pub surface: Option<&'static str>,
    /// 몸에서 떨어지는 정도: 직선·여유·볼륨. 모르면 `None`.
    pub silhouette: Option<&'static str>,
    /// 기장 묶음: 짧음(크롭·힙)·중간(허벅지·무릎)·롱(종아리·발목). 모르면 `None`.
    pub length: Option<&'static str>,
}

impl OuterLook {
    /// 기록·지시용 짧은 표기.
    pub fn label(&self) -> String {
        format!(
            "{}·{}·{}·{}",
            self.colour,
            self.surface.unwrap_or("겉감 모름"),
            self.silhouette.unwrap_or("실루엣 모름"),
            self.length.unwrap_or("기장 모름")
        )
    }

    /// 두 외투가 다른 축의 수(0~4). 모르는 축은 다르다고 세지 않는다.
    pub fn axes_apart(&self, other: &OuterLook) -> usize {
        let differ =
            |a: Option<&str>, b: Option<&str>| matches!((a, b), (Some(x), Some(y)) if x != y);
        usize::from(self.colour != other.colour)
            + usize::from(differ(self.surface, other.surface))
            + usize::from(differ(self.silhouette, other.silhouette))
            + usize::from(differ(self.length, other.length))
    }
}

/// 외투·하의의 색 묶음. 색 계열이 있으면 그것, 없으면 뉴트럴을 몇 갈래로 나눈다 — 차콜·워시드
/// 블랙·블랙은 같은 "어두운 무채색" 이다(사용자 눈에 같은 어두운 코트·바지). 올리브·카키와 인디고
/// 데님은 기본색(`mood_palette` 의 색 계열 없음)이지만 블랙과 같아 보이지 않아 따로 묶는다
/// (2026-11-18 하의 다양성: 올리브 미니스커트가 블랙 바지와 같은 색으로 세였다).
fn look_colour(c: &Clothing) -> &'static str {
    if let Some(f) = crate::services::mood_palette::colour_family(c) {
        return f;
    }
    let colour = c.color.as_deref().unwrap_or(&c.name);
    let has = |ws: &[&str]| ws.iter().any(|w| colour.contains(w));
    if has(&["올리브", "카키"]) {
        return "올리브";
    }
    if has(&["인디고", "데님", "미드블루", "라이트블루"]) {
        return "인디고";
    }
    if has(&[
        "브라운",
        "카멜",
        "탄",
        "초콜릿",
        "모카",
        "에스프레소",
        "토바코",
        "비스킷",
    ]) {
        "브라운"
    } else if has(&[
        "크림",
        "아이보리",
        "베이지",
        "화이트",
        "에크루",
        "오트밀",
        "스톤",
    ]) {
        "밝은 뉴트럴"
    } else if has(&["네이비"]) {
        "네이비"
    } else if has(&["그레이"]) && c.tone != Some(crate::models::style_vocab::Tone::Dark) {
        "그레이"
    } else {
        "어두운 무채색"
    }
}

/// 눈에 보이는 겉감. 저장된 주 소재를 먼저, 없으면 이름을 본다. 주 소재가 `down` 이면 겉은
/// 나일론·폴리 셸이다(다운은 충전재다).
fn outer_surface(c: &Clothing) -> Option<&'static str> {
    let m = c.material_primary.as_deref().unwrap_or("").to_lowercase();
    let from_material = match m.as_str() {
        "wool" | "cashmere" | "tweed" | "boucle" | "felt" => Some("울"),
        "leather" => Some("가죽"),
        "suede" => Some("스웨이드"),
        "shearling" | "faux fur" | "fur" => Some("시어링"),
        "nylon" | "polyester" | "down" => Some("나일론"),
        "denim" => Some("데님"),
        "cotton" | "canvas" | "corduroy" | "twill" => Some("코튼"),
        _ => None,
    };
    from_material.or_else(|| {
        let n = c.name.as_str();
        if ["레더", "가죽"].iter().any(|w| n.contains(w)) {
            Some("가죽")
        } else if n.contains("스웨이드") {
            Some("스웨이드")
        } else if ["패딩", "다운", "퍼퍼"].iter().any(|w| n.contains(w)) {
            Some("나일론")
        } else if n.contains("울") {
            Some("울")
        } else {
            None
        }
    })
}

fn outer_silhouette(c: &Clothing) -> Option<&'static str> {
    use crate::models::style_vocab::Silhouette;
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    if ["cocoon", "boxy", "balloon", "egg-shaped", "puffer"]
        .iter()
        .any(|w| k.contains(w))
        || ["코쿤", "박시"].iter().any(|w| c.name.contains(w))
    {
        return Some("볼륨");
    }
    match c.silhouette_volume? {
        Silhouette::Oversized => Some("볼륨"),
        Silhouette::Relaxed => Some("여유"),
        Silhouette::Slim | Silhouette::Regular => Some("직선"),
    }
}

/// 기장 묶음. 설명의 길이 낱말을 먼저 본다("boxy cropped …, high-hip length" 는 짧음).
fn outer_length(c: &Clothing) -> Option<&'static str> {
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| k.contains(w));
    if has(&[
        "ankle",
        "maxi",
        "floor-length",
        "mid-calf",
        "calf-length",
        "midi length",
    ]) {
        Some("롱")
    } else if has(&["knee", "thigh"]) {
        Some("중간")
    } else if has(&["hip", "waist-length", "cropped"]) {
        Some("짧음")
    } else if ["롱", "맥시"].iter().any(|w| c.name.contains(w)) {
        Some("롱")
    } else if ["크롭", "숏", "봄버"].iter().any(|w| c.name.contains(w)) {
        Some("짧음")
    } else {
        None
    }
}

/// 외투의 네 축. 아우터가 아니면 `None`.
pub fn outer_look(c: &Clothing) -> Option<OuterLook> {
    (c.category == "아우터").then(|| OuterLook {
        id: c.id.clone(),
        name: c.name.clone(),
        colour: look_colour(c),
        surface: outer_surface(c),
        silhouette: outer_silhouette(c),
        length: outer_length(c),
    })
}

/// 화면에 보인 코디 하나(요청의 `avoid_outfits` 에서 복원).
#[derive(Debug, Clone)]
pub struct ShownLook {
    pub signature: LookSignature,
    pub bottom_id: String,
    pub bottom_name: String,
    /// 상의 이름(없으면 `None`). 지시 문장에서 반복된 상의를 알리는 데만 쓴다.
    pub top_name: Option<String>,
    /// 보인 외투(없으면 `None`).
    pub outer: Option<OuterLook>,
    /// 보인 하의의 네 축(`bottom_look`).
    pub bottom: Option<BottomLook>,
}

/// 최근 몇 장까지 보나 — 홈 한 번에 카드 2~3장이므로 대략 최근 세 번의 요청.
///
/// `avoid_outfits` 는 **최신이 앞**이다. 홈(`HomePage::loadRecommendation`)은 새 코디를 목록 앞에
/// 붙이고(최신이 01번) 그 순서 그대로 최근 30개를 보낸다. 예전에는 끝 9장을 봐서, 이력이 9장을
/// 넘으면 가장 오래된 카드로 반복을 쟀다(2026-11-18).
pub const SHOWN_LOOK_WINDOW: usize = 9;

/// 최신 `n` 장.
fn newest(shown: &[ShownLook], n: usize) -> &[ShownLook] {
    &shown[..shown.len().min(n)]
}

/// 외투 반복을 보는 창 — 최신 두 번의 홈 요청쯤.
pub const OUTER_WINDOW: usize = 6;
/// 최근 보인 카드와 같은 외투일 때 한 장마다.
pub const SAME_OUTER_PENALTY: i32 = 3;
/// 외투의 한 축(색·겉감·실루엣·기장)이 최근 보인 외투 중 하나와 같을 때.
pub const OUTER_AXIS_PENALTY: i32 = 2;
/// 외투 반복 감점의 상한. 룩 반복 감점(`MAX_SHOWN_LOOK_PENALTY`)과 따로 묶는다.
pub const MAX_OUTER_PENALTY: i32 = 10;

/// 최근 보인 외투와 겹치는 정도의 감점(0 이상). 같은 외투 한 장마다 3, 네 축마다 최근 외투 중
/// 하나라도 같으면 2. 밝은색을 싫어하는 사용자(`allow_colour = false`)에게는 색 축을 세지 않는다.
/// 모든 후보가 같은 외투면 감점이 같아 순위는 그대로다. 후보를 빼지 않는다.
pub fn outer_repeat_penalty(outer: &OuterLook, shown: &[ShownLook], allow_colour: bool) -> i32 {
    let recent: Vec<&OuterLook> = newest(shown, OUTER_WINDOW)
        .iter()
        .filter_map(|s| s.outer.as_ref())
        .collect();
    if recent.is_empty() {
        return 0;
    }
    let same_id = recent.iter().filter(|o| o.id == outer.id).count() as i32;
    let shared = |f: fn(&OuterLook) -> Option<&'static str>| {
        f(outer).is_some_and(|v| recent.iter().any(|o| f(o) == Some(v)))
    };
    let axes = [
        allow_colour && recent.iter().any(|o| o.colour == outer.colour),
        shared(|o| o.surface),
        shared(|o| o.silhouette),
        shared(|o| o.length),
    ]
    .iter()
    .filter(|b| **b)
    .count() as i32;
    (same_id * SAME_OUTER_PENALTY + axes * OUTER_AXIS_PENALTY).min(MAX_OUTER_PENALTY)
}

/// 후보 지시에 넣을 외투 한 줄. 추운 날(코트를 여미는 기온) 후보 목록에 외투가 두 벌 이상일 때만.
/// 최근 화면에 외투가 있으면 그 외투와 반복 횟수를 알리고, 목록에서 최근과 가장 다른 외투를 최대
/// 세 벌 적는다. 기록이 없어도 다섯 후보의 외투를 한 벌에 몰지 말라고 적는다.
pub fn outer_note(
    shown: &[ShownLook],
    offered: &[&Clothing],
    allow_colour: bool,
) -> Option<String> {
    let looks: Vec<OuterLook> = offered.iter().filter_map(|c| outer_look(c)).collect();
    if looks.len() < 2 {
        return None;
    }
    let mut line = String::from("- 외투 다양성: ");
    let mut repeated: Vec<(String, usize, String)> = Vec::new();
    for o in newest(shown, OUTER_WINDOW)
        .iter()
        .filter_map(|s| s.outer.as_ref())
    {
        match repeated.iter_mut().find(|(n, _, _)| *n == o.name) {
            Some((_, k, _)) => *k += 1,
            None => repeated.push((o.name.clone(), 1, o.label())),
        }
    }
    if !repeated.is_empty() {
        line.push_str(&format!(
            "방금까지 보여 준 카드의 외투는 {} 였습니다. ",
            repeated
                .iter()
                .map(|(n, k, l)| format!("{n}({k}번, {l})"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut fresh: Vec<(i32, &OuterLook)> = looks
        .iter()
        .map(|o| (outer_repeat_penalty(o, shown, allow_colour), o))
        .collect();
    fresh.sort_by_key(|(p, _)| *p);
    let picks: Vec<String> = fresh
        .iter()
        .take(3)
        .map(|(_, o)| format!("{}({})", o.name, o.label()))
        .collect();
    line.push_str(
        "이번 5개는 외투를 한 벌에 몰지 말고, 색·겉감(울·가죽·나일론·시어링 등 눈에 보이는 겉면)·실루엣·기장이 서로 다른 외투로 나눠 쓰세요",
    );
    if !repeated.is_empty() {
        line.push_str(&format!(
            ". 목록에 있다면 최근과 다른 외투를 먼저 쓰세요: {}",
            picks.join(", ")
        ));
    }
    line.push_str(". 보온이 모자라거나 무드에 맞지 않으면 억지로 바꾸지 마세요.\n");
    Some(line)
}

/// 하의가 사람 눈에 어떻게 보이는가 — 네 축(2026-11-18 엣지: "하의가 늘 차콜·워시드 블랙·블랙의
/// 긴 바지"). 외투(`OuterLook`)와 같은 방식이다: 모르는 축은 다르다고도 같다고도 세지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BottomLook {
    pub id: String,
    pub name: String,
    /// 색 묶음(`look_colour`) — 차콜·워시드 블랙·블랙은 하나다.
    pub colour: &'static str,
    /// 바지·치마·쇼츠.
    pub kind: &'static str,
    /// 미니·무릎·미디·롱. 모르면 `None`.
    pub length: Option<&'static str>,
    /// 눈에 보이는 주 소재(가죽·데님·코듀로이·스웨이드·벨벳·울·코튼·니트·합성). 모르면 `None`.
    pub surface: Option<&'static str>,
}

impl BottomLook {
    /// 기록·지시용 짧은 표기.
    pub fn label(&self) -> String {
        format!(
            "{}·{}·{}·{}",
            self.colour,
            self.kind,
            self.length.unwrap_or("기장 모름"),
            self.surface.unwrap_or("소재 모름")
        )
    }

    /// 두 하의가 다른 축의 수(0~4). 모르는 축은 다르다고 세지 않는다.
    pub fn axes_apart(&self, other: &BottomLook) -> usize {
        let differ =
            |a: Option<&str>, b: Option<&str>| matches!((a, b), (Some(x), Some(y)) if x != y);
        usize::from(self.colour != other.colour)
            + usize::from(self.kind != other.kind)
            + usize::from(differ(self.length, other.length))
            + usize::from(differ(self.surface, other.surface))
    }

    /// 다리가 많이 드러나는 하의(미니 기장의 치마·쇼츠). 추운 날 지시·보완에서 권하지 않는다.
    pub fn bares_legs(&self) -> bool {
        self.length == Some("미니")
    }
}

fn bottom_kind(c: &Clothing) -> &'static str {
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let sub = c.sub_category.as_deref().unwrap_or("");
    let n = c.name.as_str();
    let has = |ws: &[&str]| {
        ws.iter()
            .any(|w| sub.contains(w) || n.contains(w) || k.contains(w))
    };
    if has(&["스커트", "skirt"]) {
        "치마"
    } else if has(&["쇼츠", "숏팬츠", "반바지", "버뮤다", "shorts", "bermuda"]) {
        "쇼츠"
    } else {
        "바지"
    }
}

/// 기장 묶음. 설명의 길이 낱말을 먼저, 없으면 이름을 본다. 바지는 따로 적지 않으면 롱이다
/// (발목 기장 배럴 팬츠도 롱이다 — 다리를 덮는다). 치마는 모르면 `None`.
fn bottom_length(c: &Clothing, kind: &str) -> Option<&'static str> {
    let k = c.texture_keywords.as_deref().unwrap_or("").to_lowercase();
    let n = c.name.as_str();
    let has = |ws: &[&str]| ws.iter().any(|w| k.contains(w) || n.contains(w));
    if has(&["mini", "미니", "micro"]) {
        Some("미니")
    } else if has(&["midi", "미디", "mid-calf", "calf-length"]) {
        Some("미디")
    } else if has(&["maxi", "맥시", "floor-length", "ankle-length", "롱스커트"]) {
        Some("롱")
    } else if has(&["knee-length", "knee length", "무릎", "버뮤다", "bermuda"]) {
        Some("무릎")
    } else if kind == "쇼츠" {
        Some("미니")
    } else if kind == "바지" {
        Some("롱")
    } else {
        None
    }
}

/// 눈에 보이는 주 소재. 저장된 주 소재를 먼저, 없으면 이름을 본다.
fn bottom_surface(c: &Clothing) -> Option<&'static str> {
    let m = c.material_primary.as_deref().unwrap_or("").to_lowercase();
    let from_material = match m.as_str() {
        "leather" | "faux leather" => Some("가죽"),
        "denim" => Some("데님"),
        "corduroy" => Some("코듀로이"),
        "suede" => Some("스웨이드"),
        "velvet" => Some("벨벳"),
        "wool" | "cashmere" | "tweed" | "flannel" | "boucle" | "felt" => Some("울"),
        "cotton" | "canvas" | "twill" | "linen" => Some("코튼"),
        "knit" | "rib knit" => Some("니트"),
        "polyester" | "nylon" | "satin" | "sweat" | "jersey" => Some("합성"),
        _ => None,
    };
    from_material.or_else(|| {
        let n = c.name.as_str();
        if ["레더", "가죽"].iter().any(|w| n.contains(w)) {
            Some("가죽")
        } else if ["데님", "진"].iter().any(|w| n.contains(w)) {
            Some("데님")
        } else if n.contains("코듀로이") {
            Some("코듀로이")
        } else if n.contains("스웨이드") {
            Some("스웨이드")
        } else if n.contains("벨벳") {
            Some("벨벳")
        } else if n.contains("울") {
            Some("울")
        } else {
            None
        }
    })
}

/// 하의의 네 축. 하의가 아니면 `None`.
pub fn bottom_look(c: &Clothing) -> Option<BottomLook> {
    (c.category == "하의").then(|| {
        let kind = bottom_kind(c);
        BottomLook {
            id: c.id.clone(),
            name: c.name.clone(),
            colour: look_colour(c),
            kind,
            length: bottom_length(c, kind),
            surface: bottom_surface(c),
        }
    })
}

/// 하의 반복을 보는 창 — 최신 두 번의 홈 요청쯤(외투와 같다).
pub const BOTTOM_WINDOW: usize = 6;
/// 한 축이 최근 보인 하의와 같을 때 한 장마다. 축마다 `BOTTOM_AXIS_CAP` 장까지만 센다.
pub const BOTTOM_AXIS_PENALTY: i32 = 1;
/// 한 축에서 세는 최대 장 수.
pub const BOTTOM_AXIS_CAP: i32 = 3;
/// 하의 축 반복 감점의 상한. 같은 하의(id) 반복은 룩 감점(`SAME_BOTTOM_PENALTY`)이 이미 센다.
pub const MAX_BOTTOM_PENALTY: i32 = 10;

/// 최근 보인 하의와 축이 겹치는 정도의 감점(0 이상). 네 축(색 묶음·종류·기장·소재)마다 최신
/// `BOTTOM_WINDOW` 장 중 같은 값의 장 수(최대 3)만큼 1씩. "하나라도 같으면" 이 아니라 장 수를
/// 세는 이유: 바지가 많은 옷장에서는 거의 모든 후보가 어떤 최근 카드와 한 축씩은 겹친다 — 많이
/// 반복된 축일수록 더 깎아야 차이가 난다. 밝은색을 싫어하는 사용자(`allow_colour = false`)에게는
/// 색 축을 세지 않는다. 모든 후보가 같은 처지면 순위는 그대로다. 후보를 빼지 않는다.
pub fn bottom_repeat_penalty(bottom: &BottomLook, shown: &[ShownLook], allow_colour: bool) -> i32 {
    let recent: Vec<&BottomLook> = newest(shown, BOTTOM_WINDOW)
        .iter()
        .filter_map(|s| s.bottom.as_ref())
        .collect();
    let count = |same: &dyn Fn(&BottomLook) -> bool| {
        (recent.iter().filter(|b| same(b)).count() as i32).min(BOTTOM_AXIS_CAP)
    };
    let colour = if allow_colour {
        count(&|b| b.colour == bottom.colour)
    } else {
        0
    };
    let kind = count(&|b| b.kind == bottom.kind);
    let length = bottom.length.map_or(0, |l| count(&|b| b.length == Some(l)));
    let surface = bottom
        .surface
        .map_or(0, |s| count(&|b| b.surface == Some(s)));
    ((colour + kind + length + surface) * BOTTOM_AXIS_PENALTY).min(MAX_BOTTOM_PENALTY)
}

/// 후보 지시에 넣을 하의 한 줄. 후보 목록의 하의가 두 축 이상 다른 것을 하나라도 가질 때만.
/// 최근 화면에 하의가 있으면 많이 반복된 색 묶음·종류·기장·소재를 알리고, 목록에서 최근과 가장
/// 다른 하의를 최대 세 벌 적는다. 다리가 드러나는 추운 날(`cold_legs`)에는 미니를 권하지 않는다.
pub fn bottom_note(
    shown: &[ShownLook],
    offered: &[&Clothing],
    allow_colour: bool,
    cold_legs: bool,
) -> Option<String> {
    let looks: Vec<BottomLook> = offered.iter().filter_map(|c| bottom_look(c)).collect();
    let varied = looks
        .iter()
        .any(|a| looks.iter().any(|b| a.axes_apart(b) >= 2));
    if !varied {
        return None;
    }
    let recent: Vec<&BottomLook> = newest(shown, BOTTOM_WINDOW)
        .iter()
        .filter_map(|s| s.bottom.as_ref())
        .collect();
    let mut line = String::from("- 하의 다양성: ");
    if !recent.is_empty() {
        let mut repeated: Vec<String> = Vec::new();
        let mut tally = |label: &str, values: Vec<&str>| {
            let mut counts: Vec<(&str, usize)> = Vec::new();
            for v in values {
                match counts.iter_mut().find(|(x, _)| *x == v) {
                    Some((_, k)) => *k += 1,
                    None => counts.push((v, 1)),
                }
            }
            for (v, k) in counts.into_iter().filter(|(_, k)| *k >= 2) {
                repeated.push(format!("{label} {v}({k}번)"));
            }
        };
        if allow_colour {
            tally("색", recent.iter().map(|b| b.colour).collect());
        }
        tally("종류", recent.iter().map(|b| b.kind).collect());
        tally("기장", recent.iter().filter_map(|b| b.length).collect());
        tally("소재", recent.iter().filter_map(|b| b.surface).collect());
        line.push_str(&format!(
            "방금까지 보여 준 카드의 하의는 {} 였습니다",
            recent
                .iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !repeated.is_empty() {
            line.push_str(&format!(" — 반복: {}", repeated.join(", ")));
        }
        line.push_str(". ");
    }
    line.push_str(if allow_colour {
        "차콜·워시드 블랙·블랙은 같은 어두운 색으로 봅니다. 이번 5개는 하의를 한 벌에 몰지 말고, 색·종류(바지·치마·쇼츠)·기장·눈에 보이는 소재(가죽·데님·코듀로이·스웨이드·벨벳·울 등)가 서로 다른 하의로 나눠 쓰세요"
    } else {
        "이번 5개는 하의를 한 벌에 몰지 말고, 종류(바지·치마·쇼츠)·기장·눈에 보이는 소재(가죽·데님·코듀로이·스웨이드·벨벳·울 등)가 서로 다른 하의로 나눠 쓰세요"
    });
    if !recent.is_empty() {
        let mut fresh: Vec<(i32, &BottomLook)> = looks
            .iter()
            .filter(|b| !(cold_legs && b.bares_legs()))
            .map(|b| (bottom_repeat_penalty(b, shown, allow_colour), b))
            .collect();
        fresh.sort_by_key(|(p, _)| *p);
        let picks: Vec<String> = fresh
            .iter()
            .take(3)
            .map(|(_, b)| format!("{}({})", b.name, b.label()))
            .collect();
        if !picks.is_empty() {
            line.push_str(&format!(
                ". 목록에 있다면 최근과 다른 하의를 먼저 쓰세요: {}",
                picks.join(", ")
            ));
        }
    }
    if cold_legs {
        line.push_str(". 오늘은 다리 보온이 먼저입니다 — 미니는 쓰지 말고, 치마는 부츠로 다리를 덮을 때만 쓰세요");
    }
    line.push_str(". 상의와의 균형·색 조화·무드에 맞지 않으면 억지로 바꾸지 마세요.\n");
    Some(line)
}
/// 최근 보인 카드와 같은 룩(배색·하의 형태·톤)일 때 한 장마다.
pub const SAME_LOOK_PENALTY: i32 = 5;
/// 최근 보인 카드와 같은 하의일 때 한 장마다. 하의는 이력에 대표 카드만 남아(`recent_ids`)
/// 다른 카드로 노출된 하의는 반복 감점을 받지 않았다.
pub const SAME_BOTTOM_PENALTY: i32 = 4;
/// 노출 반복 감점의 상한. 다양성을 위해 품질이 크게 낮은 후보가 오르지 않게 묶는다
/// (`cross_mood::QUALITY_MARGIN` 과 비슷한 크기).
pub const MAX_SHOWN_LOOK_PENALTY: i32 = 12;

/// 최근 보인 카드와 같아 보이는 정도에 따른 감점(0 이상). 모든 후보가 같은 처지면 순위는
/// 바뀌지 않는다 — 어두운 옷만 있는 옷장에 색을 억지로 넣지 않는다.
pub fn shown_look_penalty(sig: &LookSignature, bottom_id: &str, shown: &[ShownLook]) -> i32 {
    let recent = newest(shown, SHOWN_LOOK_WINDOW);
    let same_look = recent
        .iter()
        .filter(|s| s.signature.looks_like(sig))
        .count() as i32;
    let same_bottom = recent.iter().filter(|s| s.bottom_id == bottom_id).count() as i32;
    (same_look * SAME_LOOK_PENALTY + same_bottom * SAME_BOTTOM_PENALTY).min(MAX_SHOWN_LOOK_PENALTY)
}

/// 후보 지시에 넣을 한 줄 — 최근 보인 카드에서 같은 하의가 두 번 이상이거나 무채색 상의 + 어두운
/// 하의 룩이 두 번 이상일 때만. 그렇지 않으면 `None`(지시를 바꾸지 않는다). 밝은색을 싫어한다고
/// 적은 사용자(`allow_colour = false`)에게는 색을 권하지 않고 하의 형태만 말한다.
pub fn repetition_note(shown: &[ShownLook], allow_colour: bool) -> Option<String> {
    let recent = newest(shown, SHOWN_LOOK_WINDOW);
    let repeated_of = |names: Vec<&str>| -> Vec<String> {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for n in names {
            match counts.iter_mut().find(|(m, _)| *m == n) {
                Some((_, k)) => *k += 1,
                None => counts.push((n, 1)),
            }
        }
        counts
            .iter()
            .filter(|(_, k)| *k >= 2)
            .map(|(n, k)| format!("{n}({k}번)"))
            .collect()
    };
    let repeated = repeated_of(recent.iter().map(|s| s.bottom_name.as_str()).collect());
    let repeated_tops = repeated_of(
        recent
            .iter()
            .filter_map(|s| s.top_name.as_deref())
            .collect(),
    );
    let dark_plain = recent
        .iter()
        .filter(|s| s.signature.achromatic() && s.signature.bottom_dark)
        .count();
    let dark_plain = if allow_colour { dark_plain } else { 0 };
    if repeated.is_empty() && repeated_tops.is_empty() && dark_plain < 2 {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if !repeated.is_empty() {
        parts.push(format!("하의 {}", repeated.join(", ")));
    }
    if !repeated_tops.is_empty() {
        parts.push(format!("상의 {}", repeated_tops.join(", ")));
    }
    let mut facts: Vec<String> = Vec::new();
    if !parts.is_empty() {
        facts.push(format!("{} 가 반복됐습니다", parts.join(", ")));
    }
    if dark_plain >= 2 {
        facts.push(format!(
            "무채색 상의 + 어두운 하의 룩이 {dark_plain}번 나왔습니다"
        ));
    }
    let mut line = format!(
        "- 최근 화면: 방금까지 보여 준 코디에서 {}",
        facts.join(" 그리고 ")
    );
    line.push_str(if allow_colour {
        ". 이번 5개는 같은 상의·하의를 되풀이하지 말고, 목록에 있다면 다른 형태의 하의(스커트·슬림·스트레이트·중간 톤 데님 등)와 다른 색 있는 상의·아우터를 먼저 쓰세요. 날씨와 무드에 맞지 않으면 억지로 바꾸지 마세요.\n"
    } else {
        ". 이번 5개는 같은 상의·하의를 되풀이하지 말고, 목록에 있다면 다른 형태의 하의(스커트·슬림·스트레이트 등)를 먼저 쓰세요. 날씨와 무드에 맞지 않으면 억지로 바꾸지 마세요.\n"
    });
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::style_vocab::Tone;

    fn item(name: &str, category: &str, color: &str, tone: Tone, sub: &str, kw: &str) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: name.into(),
            name: name.into(),
            category: category.into(),
            color: Some(color.into()),
            thickness: crate::models::style_vocab::Thickness::Medium,
            image_url: None,
            tone: Some(tone),
            saturation: None,
            style: None,
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: None,
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
            sub_category: Some(sub.into()),
            floating_score: None,
            strong_style_score: None,
            texture_keywords: Some(kw.into()),
            created_at: now,
            updated_at: now,
        }
    }

    /// 하의 형태는 저장된 속성으로 나눈다. "wide straight leg" 는 와이드다.
    #[test]
    fn bottom_shapes_come_from_stored_attributes() {
        use BottomShape::*;
        for (name, sub, kw, shape) in [
            (
                "차콜 울 배럴 레그 팬츠",
                "와이드팬츠",
                "wool blend, curved barrel leg",
                Barrel,
            ),
            (
                "차콜 울 와이드 슬랙스",
                "슬랙스",
                "wool, wide leg, pressed crease",
                Wide,
            ),
            (
                "그레이 크로스오버 데님",
                "데님팬츠",
                "asymmetric crossover waistband, wide straight leg",
                Wide,
            ),
            ("워시드 블랙 슬림 데님", "데님팬츠", "denim, slim leg", Slim),
            ("차콜 테이퍼드 울 팬츠", "슬랙스", "wool, tapered leg", Slim),
            (
                "블랙 스트레이트 울혼방 슬랙스",
                "슬랙스",
                "wool blend, straight leg",
                Straight,
            ),
            (
                "블랙 레더 미디 스커트",
                "스커트",
                "smooth matte leather, midi length",
                Skirt,
            ),
            (
                "워시드 인디고 플레어 데님",
                "플레어진",
                "washed denim, flare",
                Flare,
            ),
            (
                "카키 코튼 카고팬츠",
                "카고팬츠",
                "cotton twill, cargo pockets",
                Other,
            ),
        ] {
            assert_eq!(
                bottom_shape(&item(name, "하의", "블랙", Tone::Dark, sub, kw)),
                shape,
                "{name}"
            );
        }
    }

    /// 블랙 탱크 + 차콜 와이드 슬랙스와 블랙 니트 + 차콜 와이드 데님은 같아 보인다(무채색, 어두운
    /// 와이드). 틸 셔츠가 들어가거나 하의가 스커트면 다르다. 12°C 아래 여민 코트 속 상의 색은 안
    /// 보인다. 하의 형태를 모르면 같다고 단정하지 않는다.
    #[test]
    fn looks_compare_visible_colour_and_bottom_shape() {
        let tank = item("블랙 탱크", "상의", "블랙", Tone::Dark, "탱크탑", "ribbed");
        let knit = item("블랙 니트", "상의", "블랙", Tone::Dark, "니트", "rib knit");
        let teal = item("틸 셔츠", "상의", "틸", Tone::Mid, "셔츠", "poplin");
        let slacks = item(
            "차콜 와이드 슬랙스",
            "하의",
            "차콜",
            Tone::Dark,
            "슬랙스",
            "wide leg",
        );
        let denim = item(
            "블랙 와이드 데님",
            "하의",
            "블랙",
            Tone::Dark,
            "데님팬츠",
            "wide leg",
        );
        let skirt = item(
            "블랙 레더 스커트",
            "하의",
            "블랙",
            Tone::Dark,
            "스커트",
            "midi",
        );
        let cargo = item("블랙 카고", "하의", "블랙", Tone::Dark, "카고팬츠", "cargo");
        let coat = item("차콜 코트", "아우터", "차콜", Tone::Dark, "코트", "wool");
        let s = |t: &Clothing, b: &Clothing, o: Option<&Clothing>, temp: f64| {
            signature(Some(t), Some(b), o, Some(temp)).unwrap()
        };
        assert!(s(&tank, &slacks, None, 17.0).looks_like(&s(&knit, &denim, None, 17.0)));
        assert!(s(&tank, &slacks, None, 17.0).achromatic());
        assert!(!s(&tank, &slacks, None, 17.0).looks_like(&s(&teal, &slacks, None, 17.0)));
        assert!(!s(&tank, &slacks, None, 17.0).looks_like(&s(&tank, &skirt, None, 17.0)));
        assert!(s(&teal, &slacks, Some(&coat), 8.0).achromatic());
        assert!(!s(&teal, &slacks, Some(&coat), 15.0).achromatic());
        assert!(!s(&tank, &cargo, None, 17.0).looks_like(&s(&knit, &cargo, None, 17.0)));
        assert_eq!(
            s(&teal, &slacks, None, 17.0).label(),
            "청록 / 어두운 와이드"
        );
    }

    fn shown(top: &Clothing, bottom: &Clothing) -> ShownLook {
        ShownLook {
            signature: signature(Some(top), Some(bottom), None, Some(17.0)).unwrap(),
            bottom_id: bottom.id.clone(),
            bottom_name: bottom.name.clone(),
            top_name: Some(top.name.clone()),
            outer: None,
            bottom: bottom_look(bottom),
        }
    }

    /// 최근 보인 카드와 같은 룩·같은 하의일수록 깎이고 상한이 있다. 창(최근 9장) 밖은 세지 않는다.
    /// 반복이 없으면 지시 문장도 없다.
    #[test]
    fn shown_repetition_is_penalised_and_described() {
        let tank = item("블랙 탱크", "상의", "블랙", Tone::Dark, "탱크탑", "ribbed");
        let teal = item("틸 셔츠", "상의", "틸", Tone::Mid, "셔츠", "poplin");
        let barrel = item(
            "차콜 배럴 팬츠",
            "하의",
            "차콜",
            Tone::Dark,
            "와이드팬츠",
            "barrel leg",
        );
        let slacks = item(
            "차콜 와이드 슬랙스",
            "하의",
            "차콜",
            Tone::Dark,
            "슬랙스",
            "wide leg",
        );
        let skirt = item(
            "블랙 레더 스커트",
            "하의",
            "블랙",
            Tone::Dark,
            "스커트",
            "midi",
        );
        let sig =
            |t: &Clothing, b: &Clothing| signature(Some(t), Some(b), None, Some(17.0)).unwrap();

        assert_eq!(shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &[]), 0);
        assert_eq!(repetition_note(&[], true), None);

        let once = vec![shown(&teal, &skirt)];
        assert_eq!(
            shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &once),
            0
        );
        assert_eq!(repetition_note(&once, true), None);

        let history = vec![
            shown(&tank, &barrel),
            shown(&teal, &barrel),
            shown(&tank, &slacks),
        ];
        // 같은 룩 1장(무채색/어두운 배럴) + 같은 하의 2장 = 5 + 8 → 상한 12.
        assert_eq!(
            shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &history),
            MAX_SHOWN_LOOK_PENALTY
        );
        // 틸 셔츠 + 스커트는 겹치지 않는다.
        assert_eq!(
            shown_look_penalty(&sig(&teal, &skirt), &skirt.id, &history),
            0
        );
        let note = repetition_note(&history, true).unwrap();
        assert!(note.contains("차콜 배럴 팬츠(2번)"), "{note}");
        assert!(
            note.contains("무채색 상의 + 어두운 하의 룩이 2번"),
            "{note}"
        );
        assert!(note.contains("색 있는 상의"), "{note}");
        // 상의 반복(블랙 탱크 2번)도 알린다.
        assert!(note.contains("상의 블랙 탱크(2번)"), "{note}");
        // 밝은색을 싫어하는 사용자: 색을 권하지 않고 하의 형태만 말한다. 하의 반복이 없고 무채색
        // 룩만 반복됐다면 지시하지 않는다.
        let quiet = repetition_note(&history, false).unwrap();
        assert!(
            !quiet.contains("색 있는") && !quiet.contains("무채색"),
            "{quiet}"
        );
        assert!(quiet.contains("차콜 배럴 팬츠(2번)"), "{quiet}");
        let knit0 = item("블랙 니트", "상의", "블랙", Tone::Dark, "니트", "knit");
        let dark_only = vec![shown(&tank, &barrel), shown(&knit0, &slacks)];
        assert!(repetition_note(&dark_only, true).is_some());
        assert_eq!(repetition_note(&dark_only, false), None);
        // 어두운 옷만 있는 옷장: 모든 후보가 같은 룩이면 감점이 같아 순위가 바뀌지 않는다.
        let knit = item("블랙 니트", "상의", "블랙", Tone::Dark, "니트", "knit");
        assert_eq!(
            shown_look_penalty(&sig(&tank, &slacks), &slacks.id, &dark_only),
            shown_look_penalty(&sig(&knit, &slacks), &slacks.id, &dark_only)
        );

        // 최신이 앞이다(홈의 `avoid_outfits` 순서). 최신 9장 뒤의 오래된 카드는 세지 않는다.
        let mut long = vec![shown(&teal, &skirt); SHOWN_LOOK_WINDOW];
        long.extend(vec![shown(&tank, &barrel); 2]);
        assert_eq!(
            shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &long),
            0
        );
        // 반대로 최신 카드가 앞에 있으면 센다 — 예전처럼 끝(가장 오래된 쪽)을 보지 않는다.
        let mut fresh_first = vec![shown(&tank, &barrel); 2];
        fresh_first.extend(vec![shown(&teal, &skirt); SHOWN_LOOK_WINDOW]);
        assert!(shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &fresh_first) > 0);
    }

    fn outer(name: &str, color: &str, material: &str, sil: Silhouette, kw: &str) -> Clothing {
        let mut c = item(name, "아우터", color, Tone::Dark, "코트", kw);
        c.material_primary = Some(material.into());
        c.silhouette_volume = Some(sil);
        c
    }

    use crate::models::style_vocab::Silhouette;

    /// 지금 엣지 옷장의 겨울 외투를 닮은 일곱 벌(속성은 시드와 같은 낱말).
    fn edgy_winter_outers() -> Vec<Clothing> {
        use Silhouette::*;
        vec![
            outer(
                "차콜 비대칭 여밈 롱 울 코트",
                "차콜",
                "wool",
                Regular,
                "heavy wool, structured shoulders, mid-calf length, insulated lining",
            ),
            outer(
                "플럼 스트럭처드 숄더 롱 울 코트",
                "플럼",
                "wool",
                Regular,
                "heavy wool, clean straight line, ankle-length, insulated lining",
            ),
            outer(
                "코발트 하이넥 비대칭 여밈 울 코트",
                "코발트",
                "wool",
                Regular,
                "heavy wool, asymmetric wrap front, knee-length, insulated lining",
            ),
            outer(
                "블랙 레더 시어링 재킷",
                "블랙",
                "leather",
                Relaxed,
                "matte black leather, thick shearling lining, hip-length",
            ),
            outer(
                "다크브라운 스웨이드 시어링 에비에이터 재킷",
                "다크브라운",
                "suede",
                Relaxed,
                "brushed suede shell, thick shearling lining, hip-length",
            ),
            outer(
                "페트롤틸 퍼널넥 코쿤 다운 퍼퍼",
                "페트롤틸",
                "nylon",
                Oversized,
                "visible shell: matte nylon; insulation: down fill; sculpted cocoon volume, hip-length",
            ),
            outer(
                "머스타드 시어링 박시 봄버",
                "머스타드",
                "shearling",
                Relaxed,
                "visible face: dyed curly shearling pile; boxy cropped bomber cut, high-hip length",
            ),
        ]
    }

    /// 네 축은 눈에 보이는 겉감으로 읽는다 — 다운 퍼퍼는 나일론, 가죽 시어링 재킷은 가죽이다.
    #[test]
    fn outer_axes_read_the_visible_shell() {
        let o = edgy_winter_outers();
        let l = |i: usize| outer_look(&o[i]).unwrap();
        assert_eq!(l(0).label(), "어두운 무채색·울·직선·롱");
        assert_eq!(l(1).label(), "퍼플·울·직선·롱");
        assert_eq!(l(2).label(), "블루·울·직선·중간");
        assert_eq!(l(3).label(), "어두운 무채색·가죽·여유·짧음");
        assert_eq!(l(4).label(), "브라운·스웨이드·여유·짧음");
        assert_eq!(l(5).label(), "청록·나일론·볼륨·짧음");
        assert_eq!(l(6).label(), "옐로·시어링·볼륨·짧음");
        assert_eq!(l(0).axes_apart(&l(1)), 1);
        assert_eq!(l(0).axes_apart(&l(5)), 4);
        // 상의는 외투가 아니다.
        assert!(
            outer_look(&item(
                "블랙 니트",
                "상의",
                "블랙",
                Tone::Dark,
                "니트",
                "knit"
            ))
            .is_none()
        );
    }

    fn shown_with_outer(o: &Clothing) -> ShownLook {
        let tank = item("블랙 탱크", "상의", "블랙", Tone::Dark, "탱크탑", "ribbed");
        let slacks = item(
            "차콜 와이드 슬랙스",
            "하의",
            "차콜",
            Tone::Dark,
            "슬랙스",
            "wide leg",
        );
        ShownLook {
            outer: outer_look(o),
            ..shown(&tank, &slacks)
        }
    }

    /// 차콜 롱 울 코트가 세 번 보인 뒤에는 같은 외투가 가장 크게, 같은 울·직선·롱 코트(플럼)가
    /// 그다음으로 깎이고, 겉감·실루엣·기장이 다른 퍼퍼·봄버는 깎이지 않는다. 기록이 없으면 0.
    /// 밝은색을 싫어하면 색 축은 세지 않는다.
    #[test]
    fn repeated_outer_axes_are_penalised() {
        let o = edgy_winter_outers();
        let shown = vec![shown_with_outer(&o[0]); 3];
        let p = |i: usize, colour: bool| {
            outer_repeat_penalty(&outer_look(&o[i]).unwrap(), &shown, colour)
        };
        assert_eq!(p(0, true), MAX_OUTER_PENALTY);
        assert_eq!(p(1, true), 3 * OUTER_AXIS_PENALTY);
        assert_eq!(p(2, true), 2 * OUTER_AXIS_PENALTY);
        assert_eq!(p(3, true), OUTER_AXIS_PENALTY);
        assert_eq!(p(3, false), 0);
        assert_eq!(p(5, true), 0);
        assert_eq!(p(6, true), 0);
        assert_eq!(
            outer_repeat_penalty(&outer_look(&o[0]).unwrap(), &[], true),
            0
        );
        // 창(최신 6장) 뒤의 오래된 외투는 세지 않는다.
        let mut old = vec![shown_with_outer(&o[5]); OUTER_WINDOW];
        old.extend(vec![shown_with_outer(&o[0]); 3]);
        assert_eq!(
            outer_repeat_penalty(&outer_look(&o[0]).unwrap(), &old, true),
            0
        );
    }

    /// 지시 문장: 반복된 외투와 최근과 가장 다른 외투를 적는다. 기록이 없으면 외투를 나눠 쓰라는
    /// 말만. 외투가 한 벌이면 지시하지 않는다.
    #[test]
    fn outer_note_names_repeats_and_fresh_options() {
        let o = edgy_winter_outers();
        let offered: Vec<&Clothing> = o.iter().collect();
        let shown = vec![shown_with_outer(&o[0]); 3];
        let note = outer_note(&shown, &offered, true).unwrap();
        assert!(note.contains("차콜 비대칭 여밈 롱 울 코트(3번"), "{note}");
        assert!(note.contains("페트롤틸 퍼널넥 코쿤 다운 퍼퍼"), "{note}");
        assert!(note.contains("머스타드 시어링 박시 봄버"), "{note}");
        let first = outer_note(&[], &offered, true).unwrap();
        assert!(
            first.contains("한 벌에 몰지 말고") && !first.contains("먼저 쓰세요:"),
            "{first}"
        );
        assert_eq!(outer_note(&shown, &offered[..1], true), None);
    }

    fn bottom(
        name: &str,
        color: &str,
        tone: Tone,
        sub: &str,
        material: &str,
        kw: &str,
    ) -> Clothing {
        let mut c = item(name, "하의", color, tone, sub, kw);
        c.material_primary = Some(material.into());
        c
    }

    /// 지금 엣지 옷장의 하의와 데모 시드 세 벌(속성은 DB 와 같은 낱말).
    fn edgy_bottoms() -> Vec<Clothing> {
        vec![
            bottom(
                "워시드 블랙 슬림 데님",
                "워시드블랙",
                Tone::Dark,
                "데님팬츠",
                "denim",
                "denim, slim leg, washed black",
            ),
            bottom(
                "차콜 울 와이드 슬랙스",
                "차콜",
                Tone::Dark,
                "슬랙스",
                "wool",
                "wool, wide leg, pressed crease",
            ),
            bottom(
                "블랙 와이드 슬랙스",
                "블랙",
                Tone::Dark,
                "슬랙스",
                "polyester",
                "wide leg, clean drape",
            ),
            bottom(
                "블랙 레더 미디 스커트",
                "블랙",
                Tone::Dark,
                "스커트",
                "leather",
                "smooth matte leather, straight cut, midi length, back slit",
            ),
            bottom(
                "올리브 비대칭 헴 랩 미니스커트",
                "올리브",
                Tone::Mid,
                "스커트",
                "cotton",
                "cotton twill, wrap front, asymmetric hem, mini length",
            ),
            bottom(
                "인디고 와이드 데님",
                "인디고",
                Tone::Dark,
                "데님팬츠",
                "denim",
                "denim, wide leg, rigid",
            ),
            bottom(
                "그레이 워시드 크로스오버 웨이스트 데님",
                "그레이",
                Tone::Mid,
                "데님팬츠",
                "denim",
                "washed grey denim, wide straight leg, full length",
            ),
            bottom(
                "보틀그린 코듀로이 배럴 팬츠",
                "보틀그린",
                Tone::Dark,
                "와이드팬츠",
                "corduroy",
                "wide-wale cotton corduroy, curved barrel leg, full length to the ankle",
            ),
            bottom(
                "러스트 스웨이드 비대칭 랩 미디 스커트",
                "러스트",
                Tone::Mid,
                "스커트",
                "suede",
                "brushed rust suede, wrap front, asymmetric diagonal hem, midi length to mid-calf",
            ),
            bottom(
                "블랙 레더 버뮤다 쇼츠",
                "블랙",
                Tone::Dark,
                "숏팬츠",
                "leather",
                "leather, bermuda shorts, knee-length",
            ),
        ]
    }

    /// 네 축: 차콜·워시드 블랙·블랙은 한 색 묶음, 올리브·인디고는 따로다. 종류·기장·소재는 저장된
    /// 하위 분류·설명·주 소재로 읽는다.
    #[test]
    fn bottom_axes_group_near_black_and_read_kind_length_surface() {
        let b = edgy_bottoms();
        let l = |i: usize| bottom_look(&b[i]).unwrap();
        assert_eq!(l(0).label(), "어두운 무채색·바지·롱·데님");
        assert_eq!(l(1).label(), "어두운 무채색·바지·롱·울");
        assert_eq!(l(2).label(), "어두운 무채색·바지·롱·합성");
        assert_eq!(l(3).label(), "어두운 무채색·치마·미디·가죽");
        assert_eq!(l(4).label(), "올리브·치마·미니·코튼");
        assert_eq!(l(5).label(), "인디고·바지·롱·데님");
        assert_eq!(l(6).label(), "그레이·바지·롱·데님");
        assert_eq!(l(7).label(), "그린·바지·롱·코듀로이");
        assert_eq!(l(8).label(), "오렌지·치마·미디·스웨이드");
        assert_eq!(l(9).label(), "어두운 무채색·쇼츠·무릎·가죽");
        // 차콜 울과 블랙 폴리 와이드는 소재만 다르다 — 같아 보인다.
        assert_eq!(l(1).axes_apart(&l(2)), 1);
        assert_eq!(l(1).axes_apart(&l(7)), 2);
        assert_eq!(l(1).axes_apart(&l(8)), 4);
        assert!(l(4).bares_legs() && !l(8).bares_legs());
        // 외투 색 묶음도 같은 함수다: 올리브 외투는 어두운 무채색이 아니다.
        let olive = outer(
            "올리브 필드 재킷",
            "올리브",
            "cotton",
            Silhouette::Relaxed,
            "hip-length",
        );
        assert_eq!(outer_look(&olive).unwrap().colour, "올리브");
        assert!(bottom_look(&olive).is_none());
    }

    fn shown_bottom(b: &Clothing) -> ShownLook {
        let tank = item("블랙 탱크", "상의", "블랙", Tone::Dark, "탱크탑", "ribbed");
        shown(&tank, b)
    }

    /// 어두운 긴 바지가 세 번 보인 뒤: 같은 묶음의 어두운 바지가 가장 크게, 그린 코듀로이 바지는
    /// 종류·기장만, 러스트 스웨이드 미디 스커트는 거의 깎이지 않는다. 축마다 세 장까지, 상한 10.
    /// 밝은색을 싫어하면 색 축을 세지 않는다. 창(최신 6장) 뒤는 세지 않는다.
    #[test]
    fn repeated_bottom_axes_are_penalised_by_count() {
        let b = edgy_bottoms();
        let shown: Vec<ShownLook> = [0, 1, 2].iter().map(|i| shown_bottom(&b[*i])).collect();
        let p = |i: usize, colour: bool| {
            bottom_repeat_penalty(&bottom_look(&b[i]).unwrap(), &shown, colour)
        };
        // 색 3 + 바지 3 + 롱 3 + 울 1 = 10.
        assert_eq!(p(1, true), MAX_BOTTOM_PENALTY);
        // 블랙 레더 미디 스커트: 색 3.
        assert_eq!(p(3, true), 3);
        // 그린 코듀로이 바지: 바지 3 + 롱 3.
        assert_eq!(p(7, true), 6);
        assert_eq!(p(8, true), 0);
        assert_eq!(p(3, false), 0);
        assert!(p(1, true) > p(7, true) && p(7, true) > p(8, true));
        assert_eq!(
            bottom_repeat_penalty(&bottom_look(&b[1]).unwrap(), &[], true),
            0
        );
        let mut old: Vec<ShownLook> = vec![shown_bottom(&b[8]); BOTTOM_WINDOW];
        old.extend(vec![shown_bottom(&b[1]); 3]);
        assert_eq!(p(1, true), MAX_BOTTOM_PENALTY);
        assert_eq!(
            bottom_repeat_penalty(&bottom_look(&b[1]).unwrap(), &old, true),
            0
        );
    }

    /// 지시 문장: 반복된 축과 최근과 가장 다른 하의를 적는다. 추운 날에는 미니를 권하지 않고 다리
    /// 보온을 먼저 말한다. 목록 하의가 모두 거의 같으면 지시하지 않는다. 밝은색을 싫어하면 색을
    /// 말하지 않는다.
    #[test]
    fn bottom_note_names_repeats_and_fresh_options() {
        let b = edgy_bottoms();
        let offered: Vec<&Clothing> = b[..9].iter().collect();
        let shown: Vec<ShownLook> = [0, 1, 2].iter().map(|i| shown_bottom(&b[*i])).collect();
        let note = bottom_note(&shown, &offered, true, false).unwrap();
        assert!(note.contains("색 어두운 무채색(3번)"), "{note}");
        assert!(note.contains("종류 바지(3번)"), "{note}");
        assert!(
            note.contains("러스트 스웨이드 비대칭 랩 미디 스커트"),
            "{note}"
        );
        assert!(note.contains("올리브 비대칭 헴 랩 미니스커트"), "{note}");
        let cold = bottom_note(&shown, &offered, true, true).unwrap();
        assert!(!cold.contains("올리브 비대칭 헴 랩 미니스커트("), "{cold}");
        assert!(cold.contains("미니는 쓰지 말고"), "{cold}");
        let quiet = bottom_note(&shown, &offered, false, false).unwrap();
        assert!(
            !quiet.contains("색 어두운") && !quiet.contains("어두운 색으로"),
            "{quiet}"
        );
        let first = bottom_note(&[], &offered, true, false).unwrap();
        assert!(!first.contains("먼저 쓰세요:"), "{first}");
        // 차콜 울 와이드와 블랙 폴리 와이드만 있으면(한 축 차이) 지시하지 않는다.
        assert_eq!(bottom_note(&shown, &[&b[1], &b[2]], true, false), None);
    }
}
