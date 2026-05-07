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

    // 1. 톤 대비
    if a_tone != c_tone { s += 10; } else { s -= 5; }

    // 2. 색온도 믹스
    if a_temp != c_temp && a_temp != "neutral" && c_temp != "neutral" {
        s += 6;
    } else if a_temp == c_temp && a_temp != "neutral" {
        s -= 3;
    }

    // 3. 스타일 희석
    if a_style != "베이직" && c_style == "베이직" { s += 8; }
    if a_style != "베이직" && a_style == c_style { s -= 6; }

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

    // 5. 질감 연속성 (1-10 스케일)
    let tex_gap = (a_td - c_td).abs();
    if tex_gap <= 2 { s += 6; }       // 부드러운 연결
    else if tex_gap >= 5 { s -= 6; }  // 질감 단절

    // 6. 시각적 무게 밸런스
    let wt_gap = (a_vw - c_vw).abs();
    if (a_vw <= 2 && c_vw <= 2) { s -= 5; }           // 둘 다 ultra-light
    else if (a_vw >= 7 && c_vw >= 7) { s -= 3; }      // 둘 다 heavy
    else if wt_gap >= 4 && wt_gap <= 6 { s += 5; }    // 좋은 경중 대비
    else { s += 2; }

    // 7. shadow 흐름
    let shadow_pair = (a_shadow, c_shadow);
    match shadow_pair {
        ("faded", "washed") | ("washed", "faded") => s += 5,
        ("washed", "dusty") | ("dusty", "washed") => s += 4,
        ("faded", "dusty") | ("dusty", "faded") => s += 4,
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

    // 동일 색상+무드 반복 (아우터+가방, 상의+가방 등)
    let same_style_pair = a.style.as_deref() == b.style.as_deref()
        && matches!(a.style.as_deref(), Some("밀리터리") | Some("워크"));
    let same_color = a.color.as_deref().is_some()
        && a.color.as_deref() == b.color.as_deref();
    if same_style_pair && same_color {
        s -= 8; // olive outer + olive bag, navy work + navy work bag
    } else if same_style_pair {
        s -= 3; // 같은 스타일 반복 (색은 다름)
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

    // 7. A. strong_style_density — cosplay penalty 강화
    let strong_items: Vec<&&Clothing> = items.iter()
        .filter(|i| matches!(i.style.as_deref(), Some("밀리터리") | Some("워크")))
        .collect();
    let strong_count = strong_items.len();
    if strong_count >= 4 { s -= 20; }      // 거의 유니폼
    else if strong_count >= 3 { s -= 12; } // 코스프레 경계

    // D. neutralizer 체크 — strong이 2+ 이면 나머지 중 neutralizer 필요
    if strong_count >= 2 {
        let neutral_count = items.iter()
            .filter(|i| is_neutralizer(i))
            .count();
        if neutral_count == 0 {
            s -= 10; // neutralizer 없이 strong 과밀
        } else if neutral_count == 1 && strong_count >= 3 {
            s -= 5; // neutralizer 부족
        }
    }

    // C. 가방 military/tech overload — military outfit에 military bag 겹침
    let bag = items.iter().find(|i| i.category == "가방");
    if let Some(b) = bag {
        let bag_style = b.style.as_deref().unwrap_or("베이직");
        if bag_style == "밀리터리" && strong_count >= 2 {
            s -= 8; // military outfit + military bag = overload
        }
        // tech/outdoor bag + military/workwear anchor = 무드 충돌
        let bag_mat = b.material_primary.as_deref().unwrap_or("");
        if bag_mat == "nylon" && strong_count >= 2 {
            s -= 5; // tech bag + rugged outfit
        }
    }

    // 8. 체형 밸런스
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
    let name = &item.name;

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
    // faded shadow + 베이직
    if shadow == "faded" && style == "베이직" {
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
