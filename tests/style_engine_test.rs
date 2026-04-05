use chrono::NaiveDateTime;
use rust_web_app::models::clothing::Clothing;
use rust_web_app::models::outfit::{
    EvaluationResult, IssueCode, OutfitContext, OutfitSlot, SlotKind,
};
use rust_web_app::services::style_engine;

// ─── Test helper: build Clothing without needing DB ───

fn ts() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2026-04-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}

struct ItemBuilder {
    name: &'static str,
    category: &'static str,
    color: &'static str,
    tone: &'static str,       // 밝음 / 중간 / 어두움
    saturation: &'static str, // 낮음 / 중간 / 높음
    style: &'static str,
    weight: &'static str,
    role: &'static str,
    color_temp: &'static str, // warm / cool / neutral
    statement: i8,
    formality: i8,
}

impl ItemBuilder {
    fn build(&self) -> Clothing {
        Clothing {
            id: self.name.to_string(),
            name: self.name.to_string(),
            category: self.category.to_string(),
            color: Some(self.color.to_string()),
            thickness: "medium".to_string(),
            image_url: None,
            tone: Some(self.tone.to_string()),
            saturation: Some(self.saturation.to_string()),
            style: Some(self.style.to_string()),
            weight: Some(self.weight.to_string()),
            role: Some(self.role.to_string()),
            color_temperature: Some(self.color_temp.to_string()),
            versatility: Some("flexible".to_string()),
            statement_level: Some(self.statement),
            formality_level: Some(self.formality),
            created_at: ts(),
            updated_at: ts(),
        }
    }
}

fn slot(kind: SlotKind, item: &ItemBuilder, seasons: &[&str], worlds: &[&str]) -> OutfitSlot {
    OutfitSlot {
        slot: kind,
        clothing: item.build(),
        seasons: seasons.iter().map(|s| s.to_string()).collect(),
        texture_worlds: worlds.iter().map(|s| s.to_string()).collect(),
    }
}

// ─── Item definitions (matching real wardrobe) ───

fn 헤더그레이_헨리넥() -> ItemBuilder {
    ItemBuilder {
        name: "헤더그레이 헨리넥 티셔츠",
        category: "상의", color: "헤더그레이",
        tone: "중간", saturation: "낮음", style: "베이직",
        weight: "가벼움", role: "밥", color_temp: "neutral",
        statement: 1, formality: 1,
    }
}

fn 화이트_크루넥() -> ItemBuilder {
    ItemBuilder {
        name: "화이트 크루넥 티셔츠",
        category: "상의", color: "화이트",
        tone: "밝음", saturation: "낮음", style: "베이직",
        weight: "가벼움", role: "밥", color_temp: "neutral",
        statement: 1, formality: 1,
    }
}

fn 네이비_샴브레이() -> ItemBuilder {
    ItemBuilder {
        name: "네이비 샴브레이 셔츠",
        category: "상의", color: "네이비",
        tone: "어두움", saturation: "중간", style: "워크",
        weight: "중간", role: "연결템", color_temp: "cool",
        statement: 2, formality: 2,
    }
}

fn 로얄블루_워크셔츠() -> ItemBuilder {
    ItemBuilder {
        name: "로얄블루 워크 셔츠",
        category: "상의", color: "로얄블루",
        tone: "중간", saturation: "높음", style: "워크",
        weight: "중간", role: "반찬", color_temp: "cool",
        statement: 3, formality: 2,
    }
}

fn 차콜_슬랙스() -> ItemBuilder {
    ItemBuilder {
        name: "차콜 코튼 트윌 슬랙스",
        category: "하의", color: "차콜",
        tone: "어두움", saturation: "낮음", style: "포멀",
        weight: "중간", role: "구조템", color_temp: "neutral",
        statement: 1, formality: 4,
    }
}

fn 인디고_데님() -> ItemBuilder {
    ItemBuilder {
        name: "인디고 셀비지 데님",
        category: "하의", color: "인디고",
        tone: "어두움", saturation: "중간", style: "베이직",
        weight: "무거움", role: "밥", color_temp: "cool",
        statement: 2, formality: 2,
    }
}

fn 올리브_치노() -> ItemBuilder {
    ItemBuilder {
        name: "올리브 치노 팬츠",
        category: "하의", color: "올리브",
        tone: "중간", saturation: "중간", style: "베이직",
        weight: "중간", role: "연결템", color_temp: "warm",
        statement: 1, formality: 2,
    }
}

fn 카멜_치노() -> ItemBuilder {
    ItemBuilder {
        name: "카멜 브러시드 치노",
        category: "하의", color: "카멜",
        tone: "중간", saturation: "중간", style: "베이직",
        weight: "중간", role: "밥", color_temp: "warm",
        statement: 1, formality: 2,
    }
}

fn M65() -> ItemBuilder {
    ItemBuilder {
        name: "올리브 M-65 필드 자켓",
        category: "아우터", color: "올리브",
        tone: "중간", saturation: "중간", style: "밀리터리",
        weight: "무거움", role: "약한반찬", color_temp: "warm",
        statement: 3, formality: 2,
    }
}

fn 러스트_트러커() -> ItemBuilder {
    ItemBuilder {
        name: "러스트 코듀로이 트러커 자켓",
        category: "아우터", color: "러스트",
        tone: "중간", saturation: "높음", style: "워크",
        weight: "중간", role: "반찬", color_temp: "warm",
        statement: 4, formality: 2,
    }
}

fn 그레이_스니커() -> ItemBuilder {
    ItemBuilder {
        name: "그레이 스웨이드 러닝 스니커",
        category: "신발", color: "그레이",
        tone: "중간", saturation: "낮음", style: "베이직",
        weight: "중간", role: "구조템", color_temp: "neutral",
        statement: 1, formality: 1,
    }
}

fn 타우페_독일군() -> ItemBuilder {
    ItemBuilder {
        name: "타우페 독일군 스니커",
        category: "신발", color: "타우페",
        tone: "중간", saturation: "낮음", style: "베이직",
        weight: "가벼움", role: "연결템", color_temp: "warm",
        statement: 1, formality: 2,
    }
}

fn 올리브_헬멧백() -> ItemBuilder {
    ItemBuilder {
        name: "올리브 헬멧백",
        category: "가방", color: "올리브",
        tone: "중간", saturation: "중간", style: "밀리터리",
        weight: "중간", role: "약한반찬", color_temp: "warm",
        statement: 3, formality: 1,
    }
}

fn 피그먼트_백팩() -> ItemBuilder {
    ItemBuilder {
        name: "피그먼트 그레이 백팩",
        category: "가방", color: "그레이",
        tone: "중간", saturation: "낮음", style: "베이직",
        weight: "중간", role: "구조템", color_temp: "neutral",
        statement: 1, formality: 2,
    }
}

// ─── New items from expansion ───

fn 크림_와플헨리넥() -> ItemBuilder {
    ItemBuilder {
        name: "크림 와플 헨리넥",
        category: "상의", color: "크림",
        tone: "밝음", saturation: "낮음", style: "베이직",
        weight: "가벼움", role: "밥", color_temp: "warm",
        statement: 1, formality: 1,
    }
}

fn 차콜_모크넥() -> ItemBuilder {
    ItemBuilder {
        name: "차콜 모크넥 롱슬리브",
        category: "상의", color: "차콜",
        tone: "어두움", saturation: "낮음", style: "베이직",
        weight: "중간", role: "구조템", color_temp: "neutral",
        statement: 1, formality: 2,
    }
}

fn 크림_치노() -> ItemBuilder {
    ItemBuilder {
        name: "크림 치노 팬츠",
        category: "하의", color: "크림",
        tone: "밝음", saturation: "낮음", style: "베이직",
        weight: "중간", role: "밥", color_temp: "warm",
        statement: 1, formality: 2,
    }
}

fn 베이지_치노() -> ItemBuilder {
    ItemBuilder {
        name: "베이지 와이드 치노",
        category: "하의", color: "베이지",
        tone: "밝음", saturation: "낮음", style: "워크",
        weight: "중간", role: "밥", color_temp: "warm",
        statement: 1, formality: 2,
    }
}

fn 네이비_코치() -> ItemBuilder {
    ItemBuilder {
        name: "네이비 코치 자켓",
        category: "아우터", color: "네이비",
        tone: "어두움", saturation: "중간", style: "베이직",
        weight: "중간", role: "구조템", color_temp: "cool",
        statement: 2, formality: 2,
    }
}

fn 화이트_캔버스() -> ItemBuilder {
    ItemBuilder {
        name: "화이트 캔버스 스니커",
        category: "신발", color: "화이트",
        tone: "밝음", saturation: "낮음", style: "베이직",
        weight: "가벼움", role: "밥", color_temp: "neutral",
        statement: 1, formality: 1,
    }
}

fn 블랙_캔버스() -> ItemBuilder {
    ItemBuilder {
        name: "블랙 캔버스 스니커",
        category: "신발", color: "블랙",
        tone: "어두움", saturation: "낮음", style: "베이직",
        weight: "중간", role: "구조템", color_temp: "neutral",
        statement: 1, formality: 2,
    }
}

// ─── Helper: check if a problem code exists in result ───

fn has_problem(result: &EvaluationResult, code: &IssueCode) -> bool {
    result.problems.iter().any(|p| p.code == *code)
}

fn problem_codes(result: &EvaluationResult) -> Vec<&IssueCode> {
    result.problems.iter().map(|p| &p.code).collect()
}

// ══════════════════════════════════════════════════════
// 1️⃣  밥 3개 테스트 — 기본 안정성 (baseline)
// ══════════════════════════════════════════════════════

#[test]
fn test_01_baseline_bab_outfit() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &헤더그레이_헨리넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Bottom, &인디고_데님(), &["봄", "가을", "겨울"], &["workwear"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 01 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score >= 80, "기본 밥 코디 점수가 너무 낮음: {}", result.score);
    assert!(result.score <= 95, "ceiling 초과: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 2️⃣  반찬 과다 테스트
// ══════════════════════════════════════════════════════

#[test]
fn test_02_too_many_accents() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &로얄블루_워크셔츠(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Outer, &러스트_트러커(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Bottom, &올리브_치노(), &["봄", "가을"], &["military", "workwear"]),
            slot(SlotKind::Bag, &올리브_헬멧백(), &["봄", "가을", "여름", "겨울"], &["military"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 02 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score <= 70, "반찬 과다인데 점수가 너무 높음: {}", result.score);
    assert!(
        has_problem(&result, &IssueCode::TooManyAccents),
        "TooManyAccents 미감지. problems: {:?}", problem_codes(&result)
    );
}

// ══════════════════════════════════════════════════════
// 3️⃣  밀리터리 + 포멀 믹스 (텍스처 조화 검증)
// ══════════════════════════════════════════════════════

#[test]
fn test_03_military_formal_mix() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &화이트_크루넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Outer, &M65(), &["봄", "가을"], &["military", "workwear"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 03 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // military + tailoring은 밸런싱 페어로 인정 — StyleConflict 없어야 함
    assert!(
        !has_problem(&result, &IssueCode::StyleConflict),
        "military+tailoring은 충돌이 아니라 조화. StyleConflict 오감지"
    );
    assert!(result.score >= 70, "밀리터리+포멀 믹스 점수가 너무 낮음: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 4️⃣  월드 오버매칭 (올리브 x3)
// ══════════════════════════════════════════════════════

#[test]
fn test_04_world_overmatching() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &헤더그레이_헨리넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Outer, &M65(), &["봄", "가을"], &["military", "workwear"]),
            slot(SlotKind::Bottom, &올리브_치노(), &["봄", "가을"], &["military", "workwear"]),
            slot(SlotKind::Bag, &올리브_헬멧백(), &["봄", "가을", "여름", "겨울"], &["military"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 04 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score <= 75, "올리브 올리브 올리브인데 점수가 높음: {}", result.score);
    assert!(
        has_problem(&result, &IssueCode::WorldOvermatching)
            || has_problem(&result, &IssueCode::TooMuchNaturalTone),
        "월드 오버매칭 or 자연톤 과다 미감지. problems: {:?}", problem_codes(&result)
    );
}

// ══════════════════════════════════════════════════════
// 5️⃣  강한 이너 테스트 (아우터 없이 반찬 상의)
// ══════════════════════════════════════════════════════

#[test]
fn test_05_strong_inner() {
    // 로얄블루 워크셔츠(반찬)가 이너일 때 + 아우터 있으면 StrongInner 발생 가능
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &로얄블루_워크셔츠(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Outer, &네이비_코치(), &["봄", "가을"], &["minimal"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 05 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // 로얄블루(반찬, 높은 채도) + 아우터 존재 → StrongInner 예상
    assert!(
        has_problem(&result, &IssueCode::StrongInner),
        "강한 이너 미감지. problems: {:?}", problem_codes(&result)
    );
}

// ══════════════════════════════════════════════════════
// 6️⃣  구조템 부재 테스트
// ══════════════════════════════════════════════════════

#[test]
fn test_06_no_structure() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &화이트_크루넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Bottom, &카멜_치노(), &["봄", "가을"], &["minimal"]),
            slot(SlotKind::Shoes, &타우페_독일군(), &["봄", "가을", "여름", "겨울"], &["military", "minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 06 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // 밥+밥+연결템 — flat outfit 감점이 있지만 신발(연결템) 보너스로 일부 상쇄
    assert!(result.score >= 83, "밥 조합 과도 감점: {}", result.score);
    assert!(result.score <= 95, "ceiling 초과: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 7️⃣ -A  밝기 밸런스 (전부 밝음)
// ══════════════════════════════════════════════════════

#[test]
fn test_07a_all_bright() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &크림_와플헨리넥(), &["봄", "가을", "여름"], &["minimal"]),
            slot(SlotKind::Bottom, &베이지_치노(), &["봄", "가을"], &["minimal"]),
            slot(SlotKind::Shoes, &화이트_캔버스(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 07a — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // AllBright -15, 추가로 FLAT_OUTFIT이나 LackOfContrast도 가능
    assert!(result.score <= 82, "전부 밝은데 감점이 부족: {}", result.score);
    assert!(result.score >= 65, "전부 밝지만 과도 감점: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 7️⃣ -B  밝기 밸런스 (전부 어두움)
// ══════════════════════════════════════════════════════

#[test]
fn test_07b_all_dark() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &차콜_모크넥(), &["봄", "가을", "겨울"], &["minimal"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &블랙_캔버스(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 07b — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score <= 75, "전부 어두운데 점수가 높음: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 8️⃣  텍스처 월드 충돌 (sweat + tailoring)
// ══════════════════════════════════════════════════════

#[test]
fn test_08_texture_world_conflict() {
    // 러스트 트러커(workwear) + 차콜 슬랙스(tailoring) + 러닝화(minimal)
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &헤더그레이_헨리넥(), &["봄", "가을", "여름"], &["sweat"]),
            slot(SlotKind::Outer, &러스트_트러커(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 08 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // sweat + tailoring 충돌 예상
    assert!(
        has_problem(&result, &IssueCode::TextureWorldConflict),
        "텍스처 충돌 미감지. problems: {:?}", problem_codes(&result)
    );
}

// ══════════════════════════════════════════════════════
// 9️⃣  신발 규칙 (포멀 코디 + 러닝화)
// ══════════════════════════════════════════════════════

#[test]
fn test_09_shoes_mismatch() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &네이비_샴브레이(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("비즈니스".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 09 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // 비즈니스 상황에 러닝화 → 격식 미스매치 (강화됨: gap >= 1.5 → 추가 감점)
    assert!(
        has_problem(&result, &IssueCode::FormalitySituationMismatch),
        "포멀 상황에 러닝화 미감지. problems: {:?}", problem_codes(&result)
    );
    assert!(result.score <= 80, "비즈니스+러닝화인데 점수가 높음: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 🔟  가방 규칙 (포멀 코디 + 밀리터리 가방)
// ══════════════════════════════════════════════════════

#[test]
fn test_10_bag_mismatch() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &네이비_샴브레이(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Outer, &러스트_트러커(), &["봄", "가을"], &["workwear"]),
            slot(SlotKind::Bottom, &차콜_슬랙스(), &["봄", "가을", "겨울"], &["tailoring"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
            slot(SlotKind::Bag, &올리브_헬멧백(), &["봄", "가을", "여름", "겨울"], &["military"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 10 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    // 올리브 헬멧백은 약한반찬, 그레이 스니커(구조템)가 보너스를 줌
    assert!(result.score <= 80, "반찬 아우터+약한반찬 가방인데 점수가 높음: {}", result.score);
    assert!(
        has_problem(&result, &IssueCode::StyleConflict)
            || has_problem(&result, &IssueCode::BagConflict)
            || has_problem(&result, &IssueCode::SlotRoleMismatch),
        "가방 관련 문제 미감지. problems: {:?}", problem_codes(&result)
    );
}

// ══════════════════════════════════════════════════════
// 1️⃣3️⃣  경계 테스트 — 애매한 조합 (과도 감점 방지)
// ══════════════════════════════════════════════════════

#[test]
fn test_13_borderline_no_excessive_penalty() {
    // warm 중간톤 위주지만 극단적이지 않은 조합
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &헤더그레이_헨리넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Bottom, &올리브_치노(), &["봄", "가을"], &["military", "workwear"]),
            slot(SlotKind::Shoes, &타우페_독일군(), &["봄", "가을", "여름", "겨울"], &["military", "minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 13 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score >= 65, "애매한 조합인데 과도 감점: {}", result.score);
    assert!(result.score <= 90, "애매한 조합인데 과대 평가: {}", result.score);
}

// ══════════════════════════════════════════════════════
// 1️⃣4️⃣  완벽한 조합 — ceiling 95 검증
// ══════════════════════════════════════════════════════

#[test]
fn test_14_perfect_outfit_ceiling() {
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &헤더그레이_헨리넥(), &["봄", "가을", "여름"], &["minimal", "sweat"]),
            slot(SlotKind::Bottom, &인디고_데님(), &["봄", "가을", "겨울"], &["workwear"]),
            slot(SlotKind::Shoes, &그레이_스니커(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
            slot(SlotKind::Bag, &피그먼트_백팩(), &["봄", "가을", "여름", "겨울"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("봄"));

    println!("Test 14 — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(result.score >= 85, "완벽 조합이 너무 낮음: {}", result.score);
    assert!(result.score <= 95, "100점이 나오면 안 됨 (ceiling 95): {}", result.score);
}

// ══════════════════════════════════════════════════════
// 보너스: 시즌 미스매치 테스트
// ══════════════════════════════════════════════════════

#[test]
fn test_bonus_season_mismatch() {
    // 여름 옷만 입고 겨울 시즌
    let ctx = OutfitContext {
        slots: vec![
            slot(SlotKind::Top, &화이트_크루넥(), &["여름"], &["minimal", "sweat"]),
            slot(SlotKind::Bottom, &크림_치노(), &["여름"], &["minimal"]),
            slot(SlotKind::Shoes, &화이트_캔버스(), &["여름"], &["minimal"]),
        ],
        situation: Some("일상".to_string()),
    };

    let result = style_engine::evaluate(&ctx, Some("겨울"));

    println!("Test bonus — score: {}, problems: {:?}", result.score, problem_codes(&result));
    assert!(
        has_problem(&result, &IssueCode::SeasonalMismatch),
        "시즌 미스매치 미감지. problems: {:?}", problem_codes(&result)
    );
}
