//! Outfit Scorer — 3계층 물성 기반 조합 점수화.
//!
//! Layer 1: item-level complement_score (anchor 대비 개별 아이템)
//! Layer 2: pairwise_score (인접 아이템 쌍)
//! Layer 3: outfit_score (전체 조합 + 체형)

use crate::models::clothing::Clothing;
use crate::models::user_profile::UserStyleProfile;

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

    // 1. 톤 — 대비도 좋지만 continuity도 좋음. 둘 다 보상.
    if a_tone != c_tone {
        s += 4; // 대비 보너스 (약화)
    }
    // 동일 톤은 페널티가 아니라 0 (shadow continuity가 별도 보상)

    // 2. 색온도 — 동일이어도 페널티 없음 (continuity 허용)
    if a_temp != c_temp && a_temp != "neutral" && c_temp != "neutral" {
        s += 3; // 믹스 보너스 (약화)
    }

    // 3. 스타일 희석 — 강한 anchor에 베이직 후보 보너스
    if a_style != "베이직" && c_style == "베이직" { s += 8; }
    if a_style != "베이직" && a_style == c_style { s -= 8; } // 같은 강스타일 반복 강화

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

    // 5. 질감 연속성
    let tex_gap = (a_td - c_td).abs();
    if tex_gap <= 2 { s += 6; }
    else if tex_gap >= 5 { s -= 6; }

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
    }

    // 9. 데님 bridge 보너스 — 데님은 어떤 anchor에서든 강력한 bridge/neutralizer
    if is_denim {
        s += 8; // 기본: 데님은 거의 모든 코디에서 bridge/grounding 역할
        // 강한 anchor에 데님 = style dilution까지 추가
        if a_style != "베이직" {
            s += 4;
        }
        // shadow continuity (데님은 대부분 washed)
        if c_shadow == "washed" {
            s += 3;
        }
        // 질감 깊이 (데님은 texture_depth 5~6)
        if c_td >= 5 {
            s += 2;
        }
    }

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
    let a_cg = color_group(a.color.as_deref().unwrap_or(""));
    let b_cg = color_group(b.color.as_deref().unwrap_or(""));
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

fn color_group(color: &str) -> &str {
    if color.contains("네이비") || color.contains("인디고") || color.contains("잉크") { return "navy"; }
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

    // 5. 톤 편중 페널티 — 전부 같은 톤이면 대비 없음
    let tones: Vec<&str> = items.iter()
        .filter_map(|i| i.tone.as_deref())
        .collect();
    if tones.len() >= 3 {
        let first = tones[0];
        let all_same = tones.iter().all(|t| *t == first);
        if all_same { s -= 10; }
    }

    // 6. 색온도 편중 페널티
    let temps: Vec<&str> = items.iter()
        .filter_map(|i| i.color_temperature.as_deref())
        .collect();
    if temps.len() >= 3 {
        let all_warm = temps.iter().all(|t| *t == "warm");
        let all_cool = temps.iter().all(|t| *t == "cool");
        if all_warm || all_cool { s -= 8; }
    }

    // 7. strong_style_density + rugged_overload
    let strong_items: Vec<&&Clothing> = items.iter()
        .filter(|i| matches!(i.style.as_deref(), Some("밀리터리") | Some("워크")))
        .collect();
    let strong_count = strong_items.len();
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
        .map(|i| color_group(i.color.as_deref().unwrap_or("")))
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

    // repeated_color_cluster — 같은 색상군이 3개 이상
    let mut cg_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for cg in &color_groups {
        if *cg != "other" { *cg_counts.entry(cg).or_insert(0) += 1; }
    }
    for (_cg, count) in &cg_counts {
        if *count >= 3 { s -= 10; }
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

    if user.upper_body.as_deref() == Some("large") {
        if let Some(shoe) = shoes {
            let vw = shoe.visual_weight_v2.unwrap_or(3);
            if vw <= 2 { s -= 6; }
            else if vw >= 5 { s += 4; }
        }
        if user.prefers_weighted_bag {
            if let Some(b) = bag {
                if b.visual_weight_v2.unwrap_or(3) <= 2 { s -= 4; }
            }
        }
    }

    if user.calves.as_deref() == Some("thick") {
        let bottom = items.iter().find(|i| i.category == "하의");
        if let Some(b) = bottom {
            if b.silhouette_volume.as_deref() == Some("slim") { s -= 3; }
        }
    }

    s
}

// ─── 전체 조합 점수: item + pairwise + outfit ───

pub fn total_outfit_score(
    anchor: &Clothing,
    outfit: &[&Clothing],
    user: Option<&UserStyleProfile>,
) -> i32 {
    let mut total = 0;

    // Layer 1: 각 아이템의 anchor 대비 complement score
    for item in outfit {
        if item.id != anchor.id {
            total += complement_score(anchor, item);
        }
    }

    // Layer 2: 인접 쌍 pairwise
    for i in 0..outfit.len() {
        for j in (i + 1)..outfit.len() {
            total += pairwise_score(outfit[i], outfit[j]);
        }
    }

    // Layer 3: outfit-level
    total += outfit_score(outfit, user);

    total
}
