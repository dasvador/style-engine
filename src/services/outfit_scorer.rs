//! Outfit Scorer — 3계층 물성 기반 조합 점수화.
//!
//! Layer 1: item-level complement_score (anchor 대비 개별 아이템)
//! Layer 2: pairwise_score (인접 아이템 쌍)
//! Layer 3: outfit_score (전체 조합 + 체형)

use std::collections::HashMap;

use crate::models::clothing::Clothing;
use crate::models::user_profile::UserStyleProfile;

/// 3층 피드백 보정
pub struct FeedbackContext {
    /// Layer 1: 아이템별 보정 (±1씩 누적)
    pub item_adj: HashMap<String, i32>,
    /// Layer 3: reason tag 선호도 (too_military → -9, good_texture → +6 등)
    pub preference: HashMap<String, i32>,
}

impl FeedbackContext {
    pub fn empty() -> Self {
        Self {
            item_adj: HashMap::new(),
            preference: HashMap::new(),
        }
    }
}

/// 조합에서 피드백 태그를 자동 감지
fn detect_outfit_tags(outfit: &[&Clothing]) -> Vec<String> {
    let mut tags = Vec::new();

    let strong_count = outfit.iter()
        .filter(|i| matches!(i.style.as_deref(), Some("밀리터리") | Some("워크")))
        .count();
    if strong_count >= 3 { tags.push("too_military".to_string()); }

    let dark_count = outfit.iter()
        .filter(|i| i.tone.as_deref() == Some("어두움"))
        .count();
    if dark_count >= 3 { tags.push("too_dark".to_string()); }

    let light_count = outfit.iter()
        .filter(|i| i.tone.as_deref() == Some("밝음"))
        .count();
    if light_count >= 3 { tags.push("too_light".to_string()); }

    let avg_td = outfit.iter()
        .filter_map(|i| i.texture_depth_v2)
        .map(|t| t as f32)
        .sum::<f32>() / outfit.len().max(1) as f32;
    if avg_td < 3.0 { tags.push("too_flat".to_string()); }
    if avg_td >= 5.0 { tags.push("good_texture_balance".to_string()); }

    let has_denim = outfit.iter().any(|i| {
        i.material_primary.as_deref() == Some("denim") || i.name.contains("데님")
    });
    if has_denim { tags.push("good_denim_bridge".to_string()); }

    let grounding: i32 = outfit.iter()
        .filter(|i| i.category == "신발" || i.category == "가방")
        .filter_map(|i| i.grounding_score)
        .map(|g| g as i32)
        .sum();
    if grounding >= 8 { tags.push("good_grounding".to_string()); }
    if grounding <= 3 { tags.push("floating_balance".to_string()); }

    let color_groups: Vec<&str> = outfit.iter()
        .map(|i| color_group(i.color.as_deref().unwrap_or("")))
        .collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for cg in &color_groups {
        if *cg != "other" { *counts.entry(cg).or_insert(0) += 1; }
    }
    if counts.values().any(|&v| v >= 3) { tags.push("color_repetition".to_string()); }

    tags
}

// ─── Layer 1: Item-level complement score ───

pub fn complement_score(anchor: &Clothing, candidate: &Clothing) -> i32 {
    let mut s: i32 = 0;

    let a_tone = anchor.tone.as_deref().unwrap_or("중간");
    let c_tone = candidate.tone.as_deref().unwrap_or("중간");
    let a_temp = anchor.color_temperature.as_deref().unwrap_or("neutral");
    let c_temp = candidate.color_temperature.as_deref().unwrap_or("neutral");
    let a_style = anchor.style.as_deref().unwrap_or("베이직");
    let c_style = candidate.style.as_deref().unwrap_or("베이직");
    let a_role = anchor.role.as_deref().unwrap_or("밥");
    let c_role = candidate.role.as_deref().unwrap_or("밥");
    let a_vw = anchor.visual_weight_v2.unwrap_or(3);
    let c_vw = candidate.visual_weight_v2.unwrap_or(3);
    let a_td = anchor.texture_depth_v2.unwrap_or(4);
    let c_td = candidate.texture_depth_v2.unwrap_or(4);
    let a_shadow = anchor.shadow_tone.as_deref().unwrap_or("faded");
    let c_shadow = candidate.shadow_tone.as_deref().unwrap_or("faded");

    let c_mat = candidate.material_primary.as_deref().unwrap_or("");
    let is_denim = c_mat == "denim" || candidate.name.contains("데님");

    // 1. 톤 — anchor 특성에 따라 방향성 차별화
    match (a_tone, c_tone) {
        // anchor 밝음 → 어두움 후보에 강한 보너스 (깊이 필요)
        ("밝음", "어두움") => s += 8,
        ("밝음", "중간") => s += 4,
        // anchor 어두움 → 밝음 후보에 강한 보너스 (환기 필요)
        ("어두움", "밝음") => s += 8,
        ("어두움", "중간") => s += 4,
        // anchor 중간 → 밝/어 양쪽 대비
        ("중간", "밝음") | ("중간", "어두움") => s += 6,
        // 동일 톤
        _ => s -= 2,
    }

    // 2. 색온도 — anchor와 반대 온도에 강한 보너스
    match (a_temp, c_temp) {
        ("warm", "cool") | ("cool", "warm") => s += 6,  // 반대 = 강한 보너스
        ("warm", "neutral") | ("cool", "neutral") => s += 3,
        ("neutral", "warm") | ("neutral", "cool") => s += 2,
        _ if a_temp == c_temp && a_temp != "neutral" => s -= 3, // 같은 온도 몰림
        _ => {}
    }

    // 3. 스타일 희석
    if a_style != "베이직" && c_style == "베이직" { s += 8; }
    if a_style != "베이직" && a_style == c_style { s -= 8; }

    // 4. role 밸런스
    if (a_role == "반찬" || a_role == "약한반찬") && (c_role == "밥" || c_role == "연결템") {
        s += 5;
    }
    if (a_role == "반찬" || a_role == "약한반찬")
        && (c_role == "반찬" || c_role == "약한반찬")
    {
        s -= 8;
    }
    if c_role == "밥" || c_role == "연결템" { s += 3; }

    // 5. 질감 연속성 (numeric depth + keyword 기반)
    let tex_gap = (a_td - c_td).abs();
    if tex_gap <= 2 { s += 6; }
    else if tex_gap >= 5 { s -= 6; }

    // texture_keywords 공유 보너스
    let a_kw = anchor.texture_keywords.as_deref().unwrap_or("");
    let c_kw = candidate.texture_keywords.as_deref().unwrap_or("");
    let shared_tex = a_kw.split(',')
        .filter(|k| !k.is_empty())
        .any(|k| c_kw.contains(k));
    if shared_tex { s += 4; } // 같은 질감 키워드 공유 = texture continuity

    // 6. 시각적 무게 밸런스
    let wt_gap = (a_vw - c_vw).abs();
    if a_vw <= 2 && c_vw <= 2 { s -= 5; }
    else if a_vw >= 7 && c_vw >= 7 { s -= 3; }
    else if wt_gap >= 4 && wt_gap <= 6 { s += 5; }
    else { s += 2; }

    // 7. shadow 흐름 — 강화 (continuity가 contrast와 경쟁)
    let shadow_pair = (a_shadow, c_shadow);
    match shadow_pair {
        ("faded", "washed") | ("washed", "faded") => s += 7,
        ("washed", "washed") => s += 5,  // 같은 washed도 continuity
        ("washed", "dusty") | ("dusty", "washed") => s += 6,
        ("faded", "dusty") | ("dusty", "faded") => s += 5,
        ("faded", "faded") => s += 4,    // faded 통일도 OK
        ("clean", "faded") | ("faded", "clean") => s += 2,
        ("clean", "clean") => s -= 4,
        _ => {}
    }

    // 8. 접지감 (신발/가방 후보에만)
    if candidate.category == "신발" || candidate.category == "가방" {
        let ground = candidate.grounding_score.unwrap_or(3) as i32;
        if ground >= 5 { s += 3; }
        if ground <= 2 { s -= 2; }

        // floating penalty — 떠보이는 아이템 감점
        let float = candidate.floating_score.unwrap_or(3) as i32;
        if float >= 7 { s -= 4; }
    }

    // 10. strong_style_score 기반 — anchor가 강하면 중립 후보 보너스 강화
    let c_strong = candidate.strong_style_score.unwrap_or(1) as i32;
    let a_strong = anchor.strong_style_score.unwrap_or(1) as i32;
    if a_strong >= 6 && c_strong <= 2 {
        s += 4; // 강한 anchor + 중립 후보 = 좋은 희석
    }
    if a_strong >= 6 && c_strong >= 6 {
        s -= 5; // 둘 다 강함 = 과밀
    }

    // 9. 소재 다양성 보너스 — anchor와 다른 소재면 가점
    let a_mat = anchor.material_primary.as_deref().unwrap_or("");
    if a_mat != c_mat && a_mat != "cotton" && c_mat != "cotton" {
        s += 3;
    }
    // 데님은 별도 보너스 없이 자연 속성(texture/shadow)으로 경쟁

    s
}

// ─── Layer 2: Pairwise score ───

pub fn pairwise_score(a: &Clothing, b: &Clothing) -> i32 {
    let mut s = 0;
    let a_td = a.texture_depth_v2.unwrap_or(4);
    let b_td = b.texture_depth_v2.unwrap_or(4);
    let a_vw = a.visual_weight_v2.unwrap_or(3);
    let b_vw = b.visual_weight_v2.unwrap_or(3);
    let a_shadow = a.shadow_tone.as_deref().unwrap_or("faded");
    let b_shadow = b.shadow_tone.as_deref().unwrap_or("faded");

    // 질감 갭
    let tex_gap = (a_td - b_td).abs();
    if tex_gap >= 6 { s -= 8; }
    else if tex_gap <= 2 { s += 3; }

    // 무게 갭 (신발+하의 특별 케이스)
    if (a.category == "신발" && b.category == "하의")
        || (a.category == "하의" && b.category == "신발")
    {
        let wt_gap = (a_vw - b_vw).abs();
        if wt_gap >= 6 { s -= 10; }
    }

    // shadow 충돌/연결
    if a_shadow == "clean" && b_shadow == "clean" { s -= 3; }
    let faded_washed = |t: &str| t == "faded" || t == "washed" || t == "dusty";
    if faded_washed(a_shadow) && faded_washed(b_shadow) { s += 4; }

    // 소재 친화도
    let a_mat = a.material_primary.as_deref().unwrap_or("");
    let b_mat = b.material_primary.as_deref().unwrap_or("");
    if material_affinity(a_mat, b_mat) { s += 3; }

    // B. same_color_upper_penalty — 이너+아우터 동색
    let is_upper_pair = (a.category == "상의" && b.category == "아우터")
        || (a.category == "아우터" && b.category == "상의");
    if is_upper_pair {
        let a_color = a.color.as_deref().unwrap_or("");
        let b_color = b.color.as_deref().unwrap_or("");
        let same_color_group = !a_color.is_empty() && !b_color.is_empty()
            && (a_color == b_color || color_group(a_color) == color_group(b_color));
        if same_color_group {
            // texture contrast가 있으면 완화
            let tex_contrast = (a_td - b_td).abs() >= 3;
            let mat_contrast = a_mat != b_mat;
            if tex_contrast && mat_contrast {
                s -= 3; // 소재 차이가 있으면 소폭만
            } else {
                s -= 10; // 동색+동소재 → 상체 닫힘
            }
        }
    }

    // 2. repeated_color_cluster — 같은 색상군 반복 (모든 슬롯 쌍)
    // color 필드가 없으면 이름에서 추출
    let a_color_src = a.color.as_deref().unwrap_or(&a.name);
    let b_color_src = b.color.as_deref().unwrap_or(&b.name);
    let a_cg = color_group(a_color_src);
    let b_cg = color_group(b_color_src);
    if a_cg != "other" && a_cg == b_cg {
        // 같은 색상군 + 같은 강한 스타일이면 강하게
        let both_strong = matches!(a.style.as_deref(), Some("밀리터리") | Some("워크"))
            && matches!(b.style.as_deref(), Some("밀리터리") | Some("워크"));
        if both_strong {
            s -= 10; // olive fatigue + olive tote = military overload
        } else {
            s -= 5; // 같은 색 반복이지만 스타일은 다름
        }
    }

    // sub_category 기반 충돌 — 같은 rugged sub_category 반복
    let a_sub = a.sub_category.as_deref().unwrap_or("");
    let b_sub = b.sub_category.as_deref().unwrap_or("");
    let rugged_subs = ["cargo","field_jacket","work_boots","deck","bdu"];
    let a_rugged = rugged_subs.contains(&a_sub);
    let b_rugged = rugged_subs.contains(&b_sub);
    if a_rugged && b_rugged { s -= 6; }

    // texture_keywords 공유 보너스 (pairwise)
    let a_kw = a.texture_keywords.as_deref().unwrap_or("");
    let b_kw = b.texture_keywords.as_deref().unwrap_or("");
    let kw_shared = a_kw.split(',')
        .filter(|k| !k.is_empty())
        .any(|k| b_kw.contains(k));
    if kw_shared { s += 2; }

    // 9. tech_vs_workwear_conflict — rolltop/nylon bag + fatigue/cargo/workwear
    let a_is_tech = a.material_primary.as_deref() == Some("nylon")
        || a.name.contains("롤탑");
    let b_is_tech = b.material_primary.as_deref() == Some("nylon")
        || b.name.contains("롤탑");
    let a_is_rugged = matches!(a.style.as_deref(), Some("밀리터리") | Some("워크"))
        || a.name.contains("카고") || a.name.contains("퍼티그") || a.name.contains("파티그");
    let b_is_rugged = matches!(b.style.as_deref(), Some("밀리터리") | Some("워크"))
        || b.name.contains("카고") || b.name.contains("퍼티그") || b.name.contains("파티그");
    if (a_is_tech && b_is_rugged) || (b_is_tech && a_is_rugged) {
        s -= 6; // tech + rugged = 무드 충돌
    }

    s
}

pub fn color_group(color: &str) -> &str {
    if color.is_empty() { return "other"; }
    if color.contains("네이비") || color.contains("인디고") || color.contains("잉크") || color.contains("다크네이비") { return "navy"; }
    if color.contains("올리브") || color.contains("카키") { return "olive"; }
    if color.contains("차콜") || color.contains("블랙") { return "dark"; }
    if color.contains("크림") || color.contains("오트밀") || color.contains("화이트") || color.contains("오프") { return "cream"; }
    if color.contains("브라운") || color.contains("러스트") || color.contains("브릭") { return "brown"; }
    if color.contains("그레이") || color.contains("그레이지") { return "gray"; }
    if color.contains("베이지") || color.contains("샌드") || color.contains("스톤") { return "beige"; }
    "other"
}

fn material_affinity(a: &str, b: &str) -> bool {
    let pairs = [
        ("suede", "canvas"), ("suede", "cotton"),
        ("denim", "cotton"), ("denim", "canvas"),
        ("leather", "wool"), ("leather", "cotton"),
        ("cotton", "linen"), ("canvas", "cotton"),
        ("wool", "knit"), ("flannel", "denim"),
        ("corduroy", "cotton"), ("corduroy", "wool"),
    ];
    if a == b { return true; }
    pairs.iter().any(|(x, y)| (a == *x && b == *y) || (a == *y && b == *x))
}

// ─── Layer 3: Outfit-level score ───

pub fn outfit_score(items: &[&Clothing], user: Option<&UserStyleProfile>) -> i32 {
    let mut s = 0;

    // 1. 무게 분포 — 상반신 vs 하반신
    let upper_wt: f32 = items.iter()
        .filter(|i| i.category == "상의" || i.category == "아우터")
        .map(|i| i.visual_weight_v2.unwrap_or(3) as f32)
        .sum();
    let lower_wt: f32 = items.iter()
        .filter(|i| i.category == "하의" || i.category == "신발")
        .map(|i| i.visual_weight_v2.unwrap_or(3) as f32)
        .sum();
    let ratio = if lower_wt > 0.0 { upper_wt / lower_wt } else { 2.0 };
    if ratio > 1.8 { s -= 12; }
    else if ratio > 1.4 { s -= 6; }
    else if (0.6..=1.3).contains(&ratio) { s += 5; }

    // 2. 접지감
    let grounding: i32 = items.iter()
        .filter(|i| i.category == "신발" || i.category == "가방")
        .filter_map(|i| i.grounding_score)
        .map(|g| g as i32)
        .sum();
    if grounding <= 3 { s -= 8; }
    else if grounding >= 8 { s += 5; }

    // 3. shadow 연속성
    let shadow_count = items.iter()
        .filter(|i| matches!(i.shadow_tone.as_deref(), Some("faded" | "washed" | "dusty")))
        .count();
    if items.len() > 0 {
        let ratio = shadow_count as f32 / items.len() as f32;
        if ratio >= 0.6 { s += 6; }
    }
    let all_clean = items.iter()
        .all(|i| i.shadow_tone.as_deref() == Some("clean"));
    if all_clean && items.len() >= 3 { s -= 8; }

    // 4. 질감 다양성
    let textures: Vec<i8> = items.iter()
        .filter_map(|i| i.texture_depth_v2)
        .collect();
    if !textures.is_empty() {
        let avg = textures.iter().map(|t| *t as f32).sum::<f32>() / textures.len() as f32;
        if avg < 2.5 { s -= 7; }
        let range = textures.iter().max().unwrap_or(&0) - textures.iter().min().unwrap_or(&0);
        if range >= 3 && range <= 6 { s += 4; }
    }

    // 5. 톤 편중 페널티
    let tones: Vec<&str> = items.iter()
        .filter_map(|i| i.tone.as_deref())
        .collect();
    if tones.len() >= 3 {
        let first = tones[0];
        let all_same = tones.iter().all(|t| *t == first);
        if all_same { s -= 10; }
    }

    // 5b. floating 조합 penalty (outfit level)
    let total_floating: i32 = items.iter()
        .filter_map(|i| i.floating_score)
        .map(|f| f as i32)
        .sum();
    let avg_floating = total_floating as f32 / items.len().max(1) as f32;
    if avg_floating >= 5.0 { s -= 8; }  // 전체가 떠보임
    else if avg_floating >= 4.0 { s -= 4; }

    // 5c. texture_keywords variety — rich 키워드가 많으면 보너스
    let rich_keywords = ["washed","faded","slubby","melange","brushed","suede","corduroy"];
    let rich_count = items.iter()
        .filter(|i| {
            let kw = i.texture_keywords.as_deref().unwrap_or("");
            rich_keywords.iter().any(|rk| kw.contains(rk))
        })
        .count();
    if rich_count >= 3 { s += 6; }   // 질감이 살아있는 조합
    else if rich_count >= 2 { s += 3; }
    // 전부 flat/cotton만이면 penalty
    let flat_only = items.iter()
        .all(|i| {
            let kw = i.texture_keywords.as_deref().unwrap_or("cotton");
            kw == "cotton" || kw == "nylon"
        });
    if flat_only && items.len() >= 3 { s -= 6; }

    // 6. 색온도 편중 페널티
    let temps: Vec<&str> = items.iter()
        .filter_map(|i| i.color_temperature.as_deref())
        .collect();
    if temps.len() >= 3 {
        let all_warm = temps.iter().all(|t| *t == "warm");
        let all_cool = temps.iter().all(|t| *t == "cool");
        if all_warm || all_cool { s -= 8; }
    }

    // 7. strong_style_density (strong_style_score 기반으로 정밀화)
    let strong_sum: i32 = items.iter()
        .map(|i| i.strong_style_score.unwrap_or(1) as i32)
        .sum();
    let strong_items: Vec<&&Clothing> = items.iter()
        .filter(|i| i.strong_style_score.unwrap_or(1) >= 5)
        .collect();
    let strong_count = strong_items.len();
    // 전체 strong 합산이 높으면 추가 penalty
    if strong_sum >= 25 { s -= 10; }
    else if strong_sum >= 20 { s -= 5; }
    if strong_count >= 4 { s -= 25; }      // 거의 유니폼
    else if strong_count >= 3 { s -= 15; } // 코스프레 경계

    // rugged 세부 체크 (cargo + work boots + work jacket 등)
    let has_cargo = items.iter().any(|i| i.name.contains("카고") || i.name.contains("퍼티그") || i.name.contains("파티그"));
    let has_work_boots = items.iter().any(|i| i.name.contains("워크부츠"));
    let has_work_jacket = items.iter().any(|i| {
        (i.category == "아우터") && matches!(i.style.as_deref(), Some("워크") | Some("밀리터리"))
    });
    let rugged_count = has_cargo as i32 + has_work_boots as i32 + has_work_jacket as i32;
    if rugged_count >= 3 { s -= 12; }
    else if rugged_count >= 2 && strong_count >= 3 { s -= 8; }

    // neutralizer 요구 — strong anchor일 때 neutralizer 2개 필요
    if strong_count >= 2 {
        let neutral_count = items.iter()
            .filter(|i| is_neutralizer(i))
            .count();
        let required = if strong_count >= 3 { 2 } else { 1 };
        if neutral_count < required {
            s -= (required as i32 - neutral_count as i32) * 8;
        }
    }

    // 가방 overload — military outfit에 military/tech bag
    let bag = items.iter().find(|i| i.category == "가방");
    if let Some(b) = bag {
        let bag_style = b.style.as_deref().unwrap_or("베이직");
        let bag_mat = b.material_primary.as_deref().unwrap_or("");
        let bag_is_rolltop = b.name.contains("롤탑");

        if bag_style == "밀리터리" && strong_count >= 2 {
            s -= 10;
        }
        if (bag_mat == "nylon" || bag_is_rolltop) && strong_count >= 2 {
            s -= 6; // tech bag + rugged outfit
        }
    }

    // isolated_accent_penalty — 조합 내 다른 아이템과 연결 없는 accent color
    let color_groups: Vec<&str> = items.iter()
        .map(|i| color_group(i.color.as_deref().unwrap_or(&i.name)))
        .collect();
    for (idx, cg) in color_groups.iter().enumerate() {
        if *cg == "other" { continue; }
        let count = color_groups.iter().filter(|g| *g == cg).count();
        if count == 1 {
            // 이 색상군이 조합 내 유일 → accent color
            let item = items[idx];
            let is_accent_color = !matches!(*cg, "cream" | "gray" | "beige" | "dark");
            let is_bag_or_shoes = item.category == "가방" || item.category == "신발";
            if is_accent_color && is_bag_or_shoes {
                s -= 7; // 연결 없는 accent 가방/신발
            }
        }
    }

    // outfit_language_alignment — muted/dusty 조합에 clean accent 충돌
    let muted_count = items.iter()
        .filter(|i| matches!(i.shadow_tone.as_deref(), Some("faded" | "washed" | "dusty")))
        .count();
    let clean_count = items.iter()
        .filter(|i| i.shadow_tone.as_deref() == Some("clean"))
        .count();
    if items.len() >= 4 && muted_count >= 3 && clean_count == 0 {
        s += 5; // 전체가 muted 통일 = 좋은 language alignment
    }
    // muted 우세 조합에 clean accent가 하나만 있으면
    if muted_count >= 3 && clean_count == 1 {
        // clean 아이템이 accent 성격이면 penalty
        let clean_item = items.iter().find(|i| i.shadow_tone.as_deref() == Some("clean"));
        if let Some(ci) = clean_item {
            let ci_cg = color_group(ci.color.as_deref().unwrap_or(""));
            if !matches!(ci_cg, "cream" | "beige" | "gray") {
                s -= 5; // muted 조합에 sharp clean accent
            }
        }
    }

    // repeated_color_cluster — 같은 색상군이 3개 이상 강화
    let mut cg_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for cg in &color_groups {
        if *cg != "other" { *cg_counts.entry(cg).or_insert(0) += 1; }
    }
    for (cg, count) in &cg_counts {
        if *count >= 4 { s -= 20; }      // 4개 이상 = 답답
        else if *count >= 3 { s -= 12; } // 3개 = 반복 (10→12)
    }

    // anchor bridge 요구 — anchor 색상군과 같은 아이템이 최소 1개 더 있어야
    // (올리브 신발인데 올리브 연결 아이템 없으면 isolated)
    // 이건 anchor가 outfit에 있을 때만 체크
    // → complement_score에서 이미 처리되므로 여기선 skip

    // low_profile vs rugged 충돌
    let upper_strong: i32 = items.iter()
        .filter(|i| i.category == "상의" || i.category == "아우터")
        .map(|i| i.strong_style_score.unwrap_or(1) as i32)
        .sum();
    let shoe_float = items.iter()
        .filter(|i| i.category == "신발")
        .filter_map(|i| i.floating_score)
        .map(|f| f as i32)
        .sum::<i32>();
    if upper_strong >= 10 && shoe_float >= 6 {
        s -= 10; // 무거운 상체 + 가벼운 신발 = 시각적 불균형
    }

    // bag reinforcement penalty — 가방이 outfit 방향을 더 강화만 하면 감점
    if let Some(b) = items.iter().find(|i| i.category == "가방") {
        let bag_cg = color_group(b.color.as_deref().unwrap_or(&b.name));
        let bag_strong = b.strong_style_score.unwrap_or(1);
        // 가방 색상이 outfit 주류 색상과 같고 + 가방도 strong이면 reinforcement
        if let Some((&dominant_cg, &dominant_count)) = cg_counts.iter().max_by_key(|(_, c)| *c) {
            if bag_cg == dominant_cg && dominant_count >= 2 && bag_strong >= 4 {
                s -= 6; // bag이 이미 과한 방향을 더 강화
            }
        }
    }

    // 소재 다양성 — 전부 같은 소재면 단조
    let materials: Vec<&str> = items.iter()
        .filter_map(|i| i.material_primary.as_deref())
        .collect();
    let unique_mats: std::collections::HashSet<&&str> = materials.iter().collect();
    if materials.len() >= 4 && unique_mats.len() <= 2 {
        s -= 8; // 소재가 2종 이하 = 단조
    }

    // 체형 밸런스
    if let Some(profile) = user {
        s += body_balance_score(items, profile);
    }

    s
}

/// neutralizer: 코디의 힘을 빼주는 중립/부드러운 아이템
fn is_neutralizer(item: &Clothing) -> bool {
    let style = item.style.as_deref().unwrap_or("");
    let role = item.role.as_deref().unwrap_or("");
    let shadow = item.shadow_tone.as_deref().unwrap_or("");
    let mat = item.material_primary.as_deref().unwrap_or("");
    let name = &item.name;

    // 데님은 강력한 neutralizer/bridge
    if mat == "denim" || name.contains("데님") {
        return true;
    }
    // 베이직 스타일 + 밥/연결 역할
    if style == "베이직" && (role == "밥" || role == "연결템") {
        return true;
    }
    // 특정 neutralizer 패턴
    if name.contains("크림") || name.contains("오트밀") || name.contains("헤더")
        || name.contains("멜란지") || name.contains("그레이지") || name.contains("샴브레이")
        || name.contains("워시드 블랙") || name.contains("슬러브")
    {
        return true;
    }
    // faded/washed shadow + 베이직
    if (shadow == "faded" || shadow == "washed") && style == "베이직" {
        return true;
    }
    false
}

fn body_balance_score(items: &[&Clothing], user: &UserStyleProfile) -> i32 {
    let mut s = 0;
    let shoes = items.iter().find(|i| i.category == "신발");
    let bag = items.iter().find(|i| i.category == "가방");
    let bottom = items.iter().find(|i| i.category == "하의");

    // 상체 큰 유저 → 접지 필요
    if user.upper_body.as_deref() == Some("large") {
        if let Some(shoe) = shoes {
            let vw = shoe.visual_weight_v2.unwrap_or(3);
            if vw <= 2 { s -= 6; }
            else if vw >= 5 { s += 4; }

            // low_profile 신발은 가끔만 (occasional)
            if user.low_profile_only_occasional {
                let float = shoe.floating_score.unwrap_or(3);
                if float >= 7 { s -= 5; }
            }

            // medium_volume_runner 보너스
            if user.medium_volume_runner_bonus {
                let sub = shoe.sub_category.as_deref().unwrap_or("");
                if sub == "runner" && vw >= 4 && vw <= 7 { s += 3; }
            }
        }
        if user.prefers_weighted_bag {
            if let Some(b) = bag {
                if b.visual_weight_v2.unwrap_or(3) <= 2 { s -= 4; }
            }
        }
    }

    // 종아리 굵은 유저 → 슬림핏 패널티
    if user.calves.as_deref() == Some("thick") {
        if let Some(b) = bottom {
            if b.silhouette_volume.as_deref() == Some("slim") { s -= 3; }
        }
    }

    // 다리 짧은 유저 → 하체 과중 패널티 (신발이 너무 무거우면 다리가 더 짧아 보임)
    if user.leg_length.as_deref() == Some("short") {
        if let Some(shoe) = shoes {
            if shoe.visual_weight_v2.unwrap_or(3) >= 8 { s -= 3; }
        }
    }

    // 취향 보정 — 선호 아이템 보너스
    for item in items {
        let kw = item.texture_keywords.as_deref().unwrap_or("");
        let mat = item.material_primary.as_deref().unwrap_or("");
        let name = &item.name;

        if user.likes_texture_depth && item.texture_depth_v2.unwrap_or(3) >= 6 { s += 1; }
        if user.likes_melange && kw.contains("melange") { s += 2; }
        if user.likes_suede && mat == "suede" { s += 2; }
        if user.likes_washed_denim && kw.contains("washed") && mat == "denim" { s += 2; }
        if user.likes_mocha_brown && name.contains("모카") { s += 2; }
        if user.likes_heather_gray && (name.contains("헤더") || name.contains("멜란지")) { s += 2; }

        // 비선호 패널티
        if user.dislikes_flat_beige && kw == "cotton" && item.tone.as_deref() == Some("밝음")
            && item.color_temperature.as_deref() == Some("warm")
            && item.texture_depth_v2.unwrap_or(3) <= 2
        {
            s -= 3; // flat beige 비선호
        }
        if user.dislikes_bright_colors && item.saturation.as_deref() == Some("높음") {
            s -= 2;
        }
    }

    // 데님 bridge 선호
    if user.denim_bridge_bonus {
        let has_denim = items.iter().any(|i| {
            i.material_primary.as_deref() == Some("denim") || i.name.contains("데님")
        });
        if has_denim { s += 3; }
    }

    // military cosplay 비선호 강화
    if user.dislikes_military_cosplay {
        let mil_count = items.iter()
            .filter(|i| i.strong_style_score.unwrap_or(1) >= 6)
            .count();
        if mil_count >= 3 { s -= 8; }
    }

    s
}

// ═══════════════════════════════════════════════════════════════
// Stage 1: Hard Reject — 성립 안 되는 조합 즉시 탈락
// ═══════════════════════════════════════════════════════════════

pub fn is_hard_rejected(outfit: &[&Clothing]) -> bool {
    let upper_strong: i32 = outfit.iter()
        .filter(|i| i.category == "상의" || i.category == "아우터")
        .map(|i| i.strong_style_score.unwrap_or(1) as i32)
        .sum();
    let shoe_float = outfit.iter()
        .filter(|i| i.category == "신발")
        .filter_map(|i| i.floating_score)
        .max().unwrap_or(0) as i32;

    // 무거운 상체 + 떠있는 신발
    if upper_strong >= 10 && shoe_float >= 6 { return true; }

    // 같은 색상군 4개 이상
    let mut cg_counts: HashMap<&str, usize> = HashMap::new();
    for i in outfit {
        let cg = color_group(i.color.as_deref().unwrap_or(&i.name));
        if cg != "other" { *cg_counts.entry(cg).or_insert(0) += 1; }
    }
    if cg_counts.values().any(|&v| v >= 4) { return true; }

    // strong_style 4개 이상
    let strong_count = outfit.iter()
        .filter(|i| i.strong_style_score.unwrap_or(1) >= 5)
        .count();
    if strong_count >= 4 { return true; }

    // 같은 소재 4개 이상 (cotton 제외)
    let mut mat_counts: HashMap<&str, usize> = HashMap::new();
    for i in outfit {
        let m = i.material_primary.as_deref().unwrap_or("cotton");
        if m != "cotton" { *mat_counts.entry(m).or_insert(0) += 1; }
    }
    if mat_counts.values().any(|&v| v >= 4) { return true; }

    false
}

// ═══════════════════════════════════════════════════════════════
// Stage 2: Archetype — 방향성 결정 + scoring modifier
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutfitArchetype {
    SoftWorkwear,
    LightweightUtility,
    GroundedVintage,
    WashedMinimal,
    FadedIvy,
    RuggedCasual,
}

impl OutfitArchetype {
    /// anchor 특성에서 적합한 archetype 후보 — 색온도/톤도 반영
    pub fn candidates_for_anchor(anchor: &Clothing) -> Vec<Self> {
        let vw = anchor.visual_weight_v2.unwrap_or(3);
        let strong = anchor.strong_style_score.unwrap_or(1);
        let float = anchor.floating_score.unwrap_or(3);
        let tone = anchor.tone.as_deref().unwrap_or("중간");
        let temp = anchor.color_temperature.as_deref().unwrap_or("neutral");

        let mut archs = Vec::new();

        // 가벼운/떠있는 anchor
        if float >= 5 || vw <= 3 {
            if temp == "warm" {
                archs.push(Self::LightweightUtility); // warm → utility 방향
            } else if tone == "밝음" {
                archs.push(Self::FadedIvy); // 밝은 neutral/cool → ivy 방향
            } else {
                archs.push(Self::WashedMinimal);
            }
        }
        // 무거운 anchor
        if vw >= 5 {
            archs.push(Self::GroundedVintage);
        }
        if vw >= 7 {
            archs.push(Self::RuggedCasual);
        }
        // 중립 anchor
        if strong <= 3 && tone != "어두움" {
            archs.push(Self::FadedIvy);
        }
        if strong >= 4 && strong <= 7 {
            archs.push(Self::SoftWorkwear);
        }
        if archs.is_empty() {
            archs.push(Self::LightweightUtility);
        }
        archs.dedup();
        archs
    }

    /// archetype별 아이템 적합도 보너스
    pub fn item_bonus(&self, item: &Clothing) -> i32 {
        let strong = item.strong_style_score.unwrap_or(1) as i32;
        let td = item.texture_depth_v2.unwrap_or(3) as i32;
        let vw = item.visual_weight_v2.unwrap_or(3) as i32;
        let float = item.floating_score.unwrap_or(3) as i32;
        let is_neutral = is_neutralizer(item);
        let is_denim = item.material_primary.as_deref() == Some("denim") || item.name.contains("데님");

        match self {
            Self::WashedMinimal => {
                let mut s = 0;
                if strong <= 2 { s += 6; }
                if strong >= 5 { s -= 8; } // rugged 아이템 강력 감점
                if td <= 4 { s += 2; }
                if is_neutral { s += 3; }
                s
            }
            Self::LightweightUtility => {
                let mut s = 0;
                if vw <= 4 { s += 4; }
                if vw >= 6 { s -= 4; }
                if float <= 4 { s += 2; }
                if is_neutral { s += 2; }
                s
            }
            Self::GroundedVintage => {
                let mut s = 0;
                if td >= 5 { s += 4; }
                if is_denim { s += 3; }
                if vw >= 4 { s += 2; }
                s
            }
            Self::SoftWorkwear => {
                let mut s = 0;
                if is_neutral { s += 5; }
                if is_denim { s += 3; }
                if strong >= 6 { s -= 4; } // workwear 과밀 방지
                s
            }
            Self::FadedIvy => {
                let mut s = 0;
                if item.formality_level.unwrap_or(2) >= 3 { s += 3; }
                if strong >= 5 { s -= 5; }
                if is_neutral { s += 3; }
                // 밝은/중간 톤 하의 선호 (어두운 데님 독주 방지)
                if item.category == "하의" {
                    let t = item.tone.as_deref().unwrap_or("중간");
                    if t == "밝음" { s += 4; }
                    else if t == "중간" { s += 2; }
                }
                s
            }
            Self::RuggedCasual => {
                let mut s = 0;
                if td >= 5 { s += 3; }
                if vw >= 5 { s += 3; }
                if item.category == "신발" && vw >= 6 { s += 3; } // grounded shoes
                s
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Stage 3: Visual Gravity — 시각적 무게중심 평가
// ═══════════════════════════════════════════════════════════════

fn visual_gravity_score(outfit: &[&Clothing]) -> i32 {
    let upper_weight: f32 = outfit.iter()
        .filter(|i| i.category == "상의" || i.category == "아우터")
        .map(|i| i.visual_weight_v2.unwrap_or(3) as f32)
        .sum();
    let lower_weight: f32 = outfit.iter()
        .filter(|i| i.category == "하의" || i.category == "신발")
        .map(|i| i.visual_weight_v2.unwrap_or(3) as f32)
        .sum();

    let ratio = if lower_weight > 0.0 { upper_weight / lower_weight } else { 3.0 };

    let mut s = 0;
    if ratio > 2.0 { s -= 15; }       // 상체 과중
    else if ratio > 1.5 { s -= 8; }
    else if (0.6..=1.3).contains(&ratio) { s += 6; } // 안정적 균형

    // shoe grounding 직접 체크
    let shoe_ground: i32 = outfit.iter()
        .filter(|i| i.category == "신발")
        .filter_map(|i| i.grounding_score)
        .map(|g| g as i32)
        .sum();
    if shoe_ground <= 2 && upper_weight >= 6.0 { s -= 8; }

    s
}

// ═══════════════════════════════════════════════════════════════
// Diversity Penalty — 아이템 반복 사용 감점
// ═══════════════════════════════════════════════════════════════

pub struct RecentHistory {
    pub item_freq: HashMap<String, usize>,
}

impl RecentHistory {
    pub fn empty() -> Self { Self { item_freq: HashMap::new() } }
}

fn diversity_penalty(outfit: &[&Clothing], recent: &RecentHistory) -> i32 {
    let mut p = 0;
    for item in outfit {
        if let Some(&freq) = recent.item_freq.get(&item.name) {
            if freq >= 3 { p -= 10; }
            else if freq >= 2 { p -= 5; }
            else if freq >= 1 { p -= 2; }
        }
    }
    p
}

// ═══════════════════════════════════════════════════════════════
// 최종 파이프라인: reject → archetype → score → diversity
// ═══════════════════════════════════════════════════════════════

pub fn total_outfit_score(
    anchor: &Clothing,
    outfit: &[&Clothing],
    user: Option<&UserStyleProfile>,
) -> i32 {
    // Stage 1: hard reject
    if is_hard_rejected(outfit) {
        return -999;
    }

    // Stage 2: archetype scoring
    let archetypes = OutfitArchetype::candidates_for_anchor(anchor);
    let best_arch = archetypes.first().copied().unwrap_or(OutfitArchetype::LightweightUtility);
    let arch_bonus: i32 = outfit.iter()
        .map(|item| best_arch.item_bonus(item))
        .sum();

    // Stage 3: 기존 scoring (축소)
    let mut item_total = 0;
    for item in outfit {
        if item.id != anchor.id {
            item_total += complement_score(anchor, item);
        }
    }

    let mut pair_total = 0;
    for i in 0..outfit.len() {
        for j in (i + 1)..outfit.len() {
            pair_total += pairwise_score(outfit[i], outfit[j]);
        }
    }

    let outfit_total = outfit_score(outfit, user);
    let gravity = visual_gravity_score(outfit);

    // 가중치: archetype 30% + gravity 25% + outfit 20% + item 15% + pair 10%
    let weighted = (arch_bonus * 3) + (gravity * 3) + (outfit_total * 2) + item_total + pair_total;

    weighted
}

/// 피드백 + diversity 적용 버전
pub fn total_outfit_score_with_feedback(
    anchor: &Clothing,
    outfit: &[&Clothing],
    user: Option<&UserStyleProfile>,
    feedback: &FeedbackContext,
) -> i32 {
    let base = total_outfit_score(anchor, outfit, user);
    if base <= -900 { return base; } // hard rejected

    let mut total = base;

    // feedback item adjustment
    for item in outfit {
        if let Some(&adj) = feedback.item_adj.get(&item.name) {
            total += adj;
        }
    }

    // feedback tag preference
    let outfit_tags = detect_outfit_tags(outfit);
    for tag in &outfit_tags {
        if let Some(&pref) = feedback.preference.get(tag.as_str()) {
            total += pref;
        }
    }

    total
}

/// 피드백 + diversity 적용 (recent history 포함)
pub fn total_outfit_score_full(
    anchor: &Clothing,
    outfit: &[&Clothing],
    user: Option<&UserStyleProfile>,
    feedback: &FeedbackContext,
    recent: &RecentHistory,
) -> i32 {
    let base = total_outfit_score_with_feedback(anchor, outfit, user, feedback);
    if base <= -900 { return base; }
    base + diversity_penalty(outfit, recent)
}
