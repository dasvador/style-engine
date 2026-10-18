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

/// 화면에 보인 코디 하나(요청의 `avoid_outfits` 에서 복원).
#[derive(Debug, Clone)]
pub struct ShownLook {
    pub signature: LookSignature,
    pub bottom_id: String,
    pub bottom_name: String,
    /// 상의 이름(없으면 `None`). 지시 문장에서 반복된 상의를 알리는 데만 쓴다.
    pub top_name: Option<String>,
}

/// 최근 몇 장까지 보나 — 홈 한 번에 카드 2~3장이므로 대략 최근 세 번의 요청.
pub const SHOWN_LOOK_WINDOW: usize = 9;
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
    let recent = &shown[shown.len().saturating_sub(SHOWN_LOOK_WINDOW)..];
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
    let recent = &shown[shown.len().saturating_sub(SHOWN_LOOK_WINDOW)..];
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

        // 9장보다 오래된 카드는 세지 않는다.
        let mut long = vec![shown(&tank, &barrel); 2];
        long.extend(vec![shown(&teal, &skirt); SHOWN_LOOK_WINDOW]);
        assert_eq!(
            shown_look_penalty(&sig(&tank, &barrel), &barrel.id, &long),
            0
        );
    }
}
