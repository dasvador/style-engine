//! Validation case runner for v2 hard filter + subscore (S6).
//!
//! fixture:
//!   tests/fixtures/wardrobe_registry.toml — 아이템 카탈로그
//!   tests/fixtures/recommendation_cases.toml — 케이스 (ID 참조)
//!
//! 실행:
//!   cargo test --test shadow_cases -- --nocapture
//!
//! 케이스 스키마와 로딩은 tests/common/mod.rs 로 분리되어 eval_scorecard 와 공유한다.
//! 회귀 게이트가 필요하면 이 파일이 아니라 eval_scorecard 를 본다 — 여기 report_* 는
//! 진단용 출력이고 통과/실패를 판정하지 않는다.

mod common;

use common::*;

use style_engine::services::serving_ranker;
use style_engine::services::style_engine_v2::{self, TodayFitLevel};

/// 상황별 케이스 비활성화 — 출근/데이트/비즈니스는 선택 폭이 좁아져
/// 현재 단계에서는 캐주얼만으로 평가. 향후 situation-aware 로직 구현 시 해제.
const SKIP_SITUATIONS: &[&str] = &["출근", "비즈니스", "데이트"];

fn is_active(case: &TestCase) -> bool {
    !matches!(case.situation.as_deref(), Some(s) if SKIP_SITUATIONS.contains(&s))
}

// ─── Tests ───

#[test]
fn fixture_schema_is_valid() {
    let registry = load_registry();
    assert!(
        registry.items.len() >= 30,
        "expected >= 30 items in registry, got {}",
        registry.items.len()
    );

    let cases = load_cases();
    assert!(
        cases.cases.len() >= 30,
        "expected >= 30 cases, got {}",
        cases.cases.len()
    );

    for c in &cases.cases {
        assert!(!c.case_id.is_empty(), "empty case_id");
        assert!(
            matches!(
                c.expected_today_fit.as_str(),
                "Pass" | "Borderline" | "Fail"
            ),
            "{}: invalid expected_today_fit '{}'",
            c.case_id,
            c.expected_today_fit
        );
        assert!(
            matches!(
                c.expected_preference.as_str(),
                "Accept" | "Reject" | "Borderline"
            ),
            "{}: invalid expected_preference '{}'",
            c.case_id,
            c.expected_preference
        );

        // 모든 참조 item이 registry에 존재하는지 확인
        for (label, id) in [
            ("top", &c.top),
            ("bottom", &c.bottom),
            ("outer", &c.outer),
            ("shoes", &c.shoes),
            ("bag", &c.bag),
        ] {
            if !id.is_empty() {
                assert!(
                    registry.items.contains_key(id.as_str()),
                    "{}: {} references unknown item '{}'",
                    c.case_id,
                    label,
                    id
                );
            }
        }
    }
}

#[test]
fn report_hard_filter_agreement() {
    let registry = load_registry();
    let case_file = load_cases();

    let mut agree_pass = 0;
    let mut agree_fail = 0;
    let mut false_positives: Vec<(String, Vec<String>, String)> = Vec::new();
    let mut false_negatives: Vec<(String, String)> = Vec::new();

    let active_cases: Vec<&TestCase> = case_file.cases.iter().filter(|c| is_active(c)).collect();
    let skipped = case_file.cases.len() - active_cases.len();

    for case in &active_cases {
        let ctx = case_to_context(case, &registry);
        let hard = style_engine_v2::run_hard_filter(&ctx, case.current_season.as_deref());

        let actual_reasons: Vec<String> = hard
            .reasons
            .iter()
            .map(|r| reason_code(r).to_string())
            .collect();

        if hard.pass == case.expected_hard_pass {
            if hard.pass {
                agree_pass += 1;
            } else {
                agree_fail += 1;
            }
        } else if !hard.pass {
            false_positives.push((case.case_id.clone(), actual_reasons, case.notes.clone()));
        } else {
            false_negatives.push((case.case_id.clone(), case.notes.clone()));
        }
    }

    let total = active_cases.len();
    let agree = agree_pass + agree_fail;

    println!();
    println!("=== Hard Filter Agreement Report ===");
    println!("total: {total} (skipped {skipped} situation-dependent)");
    println!(
        "agree: {agree} ({:.0}%)",
        100.0 * agree as f32 / total as f32
    );
    println!("  pass: {agree_pass}  fail: {agree_fail}");
    println!(
        "false positives (engine rejects, human accepts): {}",
        false_positives.len()
    );
    for (id, reasons, notes) in &false_positives {
        println!("  X {id:12}  {reasons:?}  -- {notes}");
    }
    println!(
        "false negatives (engine passes, human rejects): {}",
        false_negatives.len()
    );
    for (id, notes) in &false_negatives {
        println!("  ! {id:12}  -- {notes}");
    }
    println!("====================================");
}

#[test]
fn report_subscore_distribution() {
    let registry = load_registry();
    let case_file = load_cases();

    let mut rows: Vec<(String, i32, [i32; 4], bool, String)> = Vec::new();

    let active_cases: Vec<&TestCase> = case_file.cases.iter().filter(|c| is_active(c)).collect();
    for case in &active_cases {
        let ctx = case_to_context(case, &registry);
        let sub = style_engine_v2::compute_subscores(&ctx, case.current_season.as_deref());
        let total = style_engine_v2::compute_style_score(&sub);
        let hard = style_engine_v2::run_hard_filter(&ctx, case.current_season.as_deref());
        rows.push((
            case.case_id.clone(),
            total,
            [sub.balance, sub.coherence, sub.utility, sub.accessory],
            hard.pass,
            case.expected_preference.clone(),
        ));
    }

    println!();
    println!("=== Subscore Detail ===");
    println!(
        "{:12} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>8}",
        "case_id", "v2", "bal", "coh", "utl", "acc", "hard", "pref"
    );
    println!("{}", "-".repeat(60));
    for (id, total, axes, hard, pref) in &rows {
        let mark = if *hard { " " } else { "X" };
        println!(
            "{:12} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>8}",
            id, total, axes[0], axes[1], axes[2], axes[3], mark, pref
        );
    }

    let totals: Vec<i32> = rows.iter().map(|r| r.1).collect();
    print_stats("style_score", &totals);
    for (i, name) in ["balance", "coherence", "utility", "accessory"]
        .iter()
        .enumerate()
    {
        let vals: Vec<i32> = rows.iter().map(|r| r.2[i]).collect();
        print_stats(name, &vals);
    }
    println!("=======================");
}

fn print_stats(label: &str, data: &[i32]) {
    let mut s = data.to_vec();
    s.sort();
    let n = s.len();
    let (min, max, med) = (s[0], s[n - 1], s[n / 2]);
    let mean = s.iter().sum::<i32>() as f32 / n as f32;
    println!("  {label:12} n={n:>2} min={min:>3} med={med:>3} avg={mean:>5.1} max={max:>3}");
}

// ─── Full analysis (전체 50건, skip 없음) ───

// 진단 리포트용 집계 구조체 — 일부 필드는 현재 출력 포맷에서 쓰이지 않는다.
#[allow(dead_code)]
struct CaseResult {
    case_id: String,
    situation: String,
    temperature: f64,
    v2: i32,
    bal: i32,
    coh: i32,
    utl: i32,
    acc: i32,
    hard_pass: bool,
    hard_reasons: Vec<String>,
    expected_hard_pass: bool,
    expected_pref: String,
    expected_today_fit: String,
    today_fit: TodayFitLevel,
    serving_adj: i32,
    serving_score: i32,
    serving_reason: String,
    notes: String,
    outfit_key: String,
}

#[test]
fn report_full_analysis() {
    let registry = load_registry();
    let case_file = load_cases();

    let mut results: Vec<CaseResult> = Vec::new();
    for case in &case_file.cases {
        let ctx = case_to_context(case, &registry);
        let temp = case.temperature_c.unwrap_or(20.0);
        let hard = style_engine_v2::run_hard_filter(&ctx, case.current_season.as_deref());
        let sub = style_engine_v2::compute_subscores(&ctx, case.current_season.as_deref());
        let v2 = style_engine_v2::compute_style_score(&sub);
        let today_fit = serving_ranker::compute_today_fit(&ctx, temp);
        let (serving_adj, serving_reason) = serving_ranker::compute_serving_adjustment(&ctx);
        let serving_score = v2 + serving_adj;
        results.push(CaseResult {
            case_id: case.case_id.clone(),
            situation: case.situation.clone().unwrap_or_default(),
            temperature: temp,
            v2,
            bal: sub.balance,
            coh: sub.coherence,
            utl: sub.utility,
            acc: sub.accessory,
            hard_pass: hard.pass,
            hard_reasons: hard
                .reasons
                .iter()
                .map(|r| reason_code(r).to_string())
                .collect(),
            expected_hard_pass: case.expected_hard_pass,
            expected_pref: case.expected_preference.clone(),
            expected_today_fit: case.expected_today_fit.clone(),
            today_fit,
            serving_adj,
            serving_score,
            serving_reason,
            notes: case.notes.clone(),
            outfit_key: build_outfit_key(case),
        });
    }

    // ═══ 1. Hard False Positive Top 10 ═══
    println!();
    println!("═══ 1. Hard False Positive (engine rejects, human accepts) ═══");
    let mut fps: Vec<&CaseResult> = results
        .iter()
        .filter(|r| !r.hard_pass && r.expected_hard_pass)
        .collect();
    fps.sort_by(|a, b| b.v2.cmp(&a.v2)); // v2 높은 순 (가장 억울한 순)
    println!(
        "{:12} {:>5} {:>4} {:>4} {:>4} {:>4}  {:<30} notes",
        "case_id", "v2", "bal", "coh", "utl", "acc", "reasons"
    );
    println!("{}", "-".repeat(100));
    for (i, r) in fps.iter().enumerate().take(10) {
        println!(
            "{:12} {:>5} {:>4} {:>4} {:>4} {:>4}  {:<30} {}",
            r.case_id,
            r.v2,
            r.bal,
            r.coh,
            r.utl,
            r.acc,
            r.hard_reasons.join(","),
            truncate(&r.notes, 40),
        );
        if i >= 9 {
            break;
        }
    }
    println!("total false positives: {}", fps.len());

    // ═══ 2. Accept인데 하위 랭크 ═══
    println!();
    println!("═══ 2. Expected Accept but low v2 score ═══");
    let mut accepts: Vec<&CaseResult> = results
        .iter()
        .filter(|r| r.expected_pref == "Accept")
        .collect();
    accepts.sort_by_key(|r| r.v2);
    println!(
        "{:12} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>8}  notes",
        "case_id", "v2", "bal", "coh", "utl", "acc", "hard", "sit"
    );
    println!("{}", "-".repeat(105));
    for r in accepts.iter().take(10) {
        let hm = if r.hard_pass { " " } else { "X" };
        println!(
            "{:12} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>8}  {}",
            r.case_id,
            r.v2,
            r.bal,
            r.coh,
            r.utl,
            r.acc,
            hm,
            if r.situation.is_empty() {
                "-"
            } else {
                &r.situation
            },
            truncate(&r.notes, 40),
        );
    }

    // ═══ 3. Situation 바뀌면 결과 달라지는 케이스 ═══
    println!();
    println!("═══ 3. Same outfit, different situation → different result ═══");
    // outfit_key가 같은 케이스 쌍 찾기
    let mut by_outfit: std::collections::HashMap<&str, Vec<&CaseResult>> =
        std::collections::HashMap::new();
    for r in &results {
        by_outfit.entry(r.outfit_key.as_str()).or_default().push(r);
    }
    println!(
        "{:30} {:12} {:>8} {:>5} {:>4} {:>8}  {:12} {:>8} {:>5} {:>4} {:>8}",
        "outfit",
        "case_A",
        "sit_A",
        "v2_A",
        "hard",
        "pref_A",
        "case_B",
        "sit_B",
        "v2_B",
        "hard",
        "pref_B"
    );
    println!("{}", "-".repeat(130));
    for (key, group) in &by_outfit {
        if group.len() < 2 {
            continue;
        }
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let a = group[i];
                let b = group[j];
                if a.situation == b.situation {
                    continue;
                }
                let pref_diff = a.expected_pref != b.expected_pref
                    || a.hard_pass != b.hard_pass
                    || (a.v2 - b.v2).abs() >= 5;
                if !pref_diff {
                    continue;
                }
                let ha = if a.hard_pass { " " } else { "X" };
                let hb = if b.hard_pass { " " } else { "X" };
                println!(
                    "{:30} {:12} {:>8} {:>5} {:>4} {:>8}  {:12} {:>8} {:>5} {:>4} {:>8}",
                    truncate(key, 30),
                    a.case_id,
                    a.situation,
                    a.v2,
                    ha,
                    a.expected_pref,
                    b.case_id,
                    b.situation,
                    b.v2,
                    hb,
                    b.expected_pref,
                );
            }
        }
    }
    // 동일 outfit은 없지만 "거의 같은 조합 + situation만 다른" 쌍도 보여주기 (HC027/028 등)
    let notable_pairs = [
        ("HC027", "HC028"),
        ("HC003", "HC004"),
        ("HC031", "HC032"),
        ("HC039", "HC040"),
    ];
    println!();
    println!("--- notable pairs (similar outfit, different situation or accessory) ---");
    println!(
        "{:12} {:>8} {:>5} {:>4} {:>8}  vs  {:12} {:>8} {:>5} {:>4} {:>8}",
        "case_A", "sit_A", "v2_A", "hard", "pref_A", "case_B", "sit_B", "v2_B", "hard", "pref_B"
    );
    println!("{}", "-".repeat(110));
    for (id_a, id_b) in &notable_pairs {
        let a = results.iter().find(|r| r.case_id == *id_a);
        let b = results.iter().find(|r| r.case_id == *id_b);
        if let (Some(a), Some(b)) = (a, b) {
            let ha = if a.hard_pass { " " } else { "X" };
            let hb = if b.hard_pass { " " } else { "X" };
            println!(
                "{:12} {:>8} {:>5} {:>4} {:>8}  vs  {:12} {:>8} {:>5} {:>4} {:>8}",
                a.case_id,
                a.situation,
                a.v2,
                ha,
                a.expected_pref,
                b.case_id,
                b.situation,
                b.v2,
                hb,
                b.expected_pref,
            );
        }
    }

    // ═══ 4. Today Fit agreement ═══
    println!();
    println!("═══ 4. Today Fit Agreement ═══");
    let fit_label = |f: &TodayFitLevel| match f {
        TodayFitLevel::Pass => "Pass",
        TodayFitLevel::Borderline => "Borderline",
        TodayFitLevel::Fail => "Fail",
    };
    let mut fit_agree = 0;
    let mut fit_mismatch: Vec<(String, String, String, String, String)> = Vec::new();
    for r in &results {
        let actual = fit_label(&r.today_fit);
        if actual == r.expected_today_fit {
            fit_agree += 1;
        } else {
            fit_mismatch.push((
                r.case_id.clone(),
                r.situation.clone(),
                r.expected_today_fit.clone(),
                actual.to_string(),
                truncate(&r.notes, 40),
            ));
        }
    }
    let total = results.len();
    println!(
        "total={total}  agree={fit_agree} ({:.0}%)",
        100.0 * fit_agree as f32 / total as f32
    );
    if !fit_mismatch.is_empty() {
        println!(
            "{:12} {:>8} {:>10} {:>10} notes",
            "case_id", "sit", "expected", "actual"
        );
        for (id, sit, exp, act, notes) in &fit_mismatch {
            println!("{:12} {:>8} {:>10} {:>10} {}", id, sit, exp, act, notes);
        }
    }

    // ═══ 5. Serving score detail (situation-dependent pairs) ═══
    println!();
    println!("═══ 5. Serving Score (notable pairs) ═══");
    let notable = [
        ("HC027", "HC028"),
        ("HC003", "HC004"),
        ("HC031", "HC032"),
        ("HC039", "HC040"),
        ("HC023", "HC024"),
    ];
    println!(
        "{:12} {:>8} {:>5} {:>5} {:>4} {:>10} {:>8}  vs  {:12} {:>8} {:>5} {:>5} {:>4} {:>10} {:>8}",
        "case_A",
        "sit",
        "v2",
        "serv",
        "adj",
        "fit",
        "pref",
        "case_B",
        "sit",
        "v2",
        "serv",
        "adj",
        "fit",
        "pref"
    );
    println!("{}", "-".repeat(140));
    for (a_id, b_id) in &notable {
        let a = results.iter().find(|r| r.case_id == *a_id);
        let b = results.iter().find(|r| r.case_id == *b_id);
        if let (Some(a), Some(b)) = (a, b) {
            println!(
                "{:12} {:>8} {:>5} {:>5} {:>4} {:>10} {:>8}  vs  {:12} {:>8} {:>5} {:>5} {:>4} {:>10} {:>8}",
                a.case_id,
                a.situation,
                a.v2,
                a.serving_score,
                a.serving_adj,
                fit_label(&a.today_fit),
                a.expected_pref,
                b.case_id,
                b.situation,
                b.v2,
                b.serving_score,
                b.serving_adj,
                fit_label(&b.today_fit),
                b.expected_pref,
            );
        }
    }

    // ═══ 6. Category-filtered rates ═══
    println!();
    println!("═══ 6. Category-filtered Rates ═══");
    for (prefix, label) in [
        ("SG", "situation"),
        ("AC", "accessory"),
        ("TG", "temperature"),
    ] {
        let cat: Vec<&CaseResult> = results
            .iter()
            .filter(|r| r.case_id.starts_with(prefix))
            .collect();
        if cat.is_empty() {
            continue;
        }
        let n = cat.len();
        let h_ok = cat
            .iter()
            .filter(|r| r.hard_pass == r.expected_hard_pass)
            .count();
        let f_ok = cat
            .iter()
            .filter(|r| fit_label(&r.today_fit) == r.expected_today_fit)
            .count();
        let c_ok = cat
            .iter()
            .filter(|r| {
                r.hard_pass == r.expected_hard_pass
                    && fit_label(&r.today_fit) == r.expected_today_fit
            })
            .count();
        println!(
            "  {label:12} n={n:>2}  hard={h_ok}/{n} ({:.0}%)  fit={f_ok}/{n} ({:.0}%)  combined={c_ok}/{n} ({:.0}%)",
            100.0 * h_ok as f32 / n as f32,
            100.0 * f_ok as f32 / n as f32,
            100.0 * c_ok as f32 / n as f32
        );
        // detail mismatches
        for r in &cat {
            let h_match = r.hard_pass == r.expected_hard_pass;
            let f_match = fit_label(&r.today_fit) == r.expected_today_fit;
            if !h_match || !f_match {
                let fit_note = if f_match {
                    String::new()
                } else {
                    format!("(exp:{})", r.expected_today_fit)
                };
                println!(
                    "    {:8} hard:{}{} fit:{}{} serv={:>4} {}",
                    r.case_id,
                    if r.hard_pass { "P" } else { "F" },
                    if h_match { "" } else { "!" },
                    fit_label(&r.today_fit),
                    fit_note,
                    r.serving_score,
                    truncate(&r.notes, 35)
                );
            }
        }
    }

    // ═══ 7. HC024 cluster — 데이트+스포츠슈즈 ═══
    println!();
    println!("═══ 7. 데이트+스포츠슈즈 cluster ═══");
    let date_sport: Vec<&CaseResult> = results
        .iter()
        .filter(|r| {
            r.situation == "데이트" && r.notes.contains("러닝")
                || r.case_id == "HC024"
                || r.case_id == "AC005"
                || r.case_id == "AC014"
                || r.case_id == "SG005"
        })
        .collect();
    println!(
        "{:8} {:>5} {:>5} {:>10} {:>8} notes",
        "case_id", "v2", "serv", "fit", "pref"
    );
    for r in &date_sport {
        println!(
            "{:8} {:>5} {:>5} {:>10} {:>8} {}",
            r.case_id,
            r.v2,
            r.serving_score,
            fit_label(&r.today_fit),
            r.expected_pref,
            truncate(&r.notes, 40)
        );
    }

    // ═══ Summary ═══
    println!();
    println!("═══ Summary ═══");
    let fp = results
        .iter()
        .filter(|r| !r.hard_pass && r.expected_hard_pass)
        .count();
    let fn_ = results
        .iter()
        .filter(|r| r.hard_pass && !r.expected_hard_pass)
        .count();
    let hard_agree = total - fp - fn_;
    let full_agree = results
        .iter()
        .filter(|r| {
            r.hard_pass == r.expected_hard_pass && fit_label(&r.today_fit) == r.expected_today_fit
        })
        .count();
    println!("total={total}");
    println!(
        "hard: agree={hard_agree} ({:.0}%)  FP={fp}  FN={fn_}",
        100.0 * hard_agree as f32 / total as f32
    );
    println!(
        "today_fit: agree={fit_agree} ({:.0}%)",
        100.0 * fit_agree as f32 / total as f32
    );
    println!(
        "combined (hard+fit): agree={full_agree} ({:.0}%)",
        100.0 * full_agree as f32 / total as f32
    );
}
