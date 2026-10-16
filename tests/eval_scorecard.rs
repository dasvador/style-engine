//! 스타일 엔진 eval 하네스 — 회귀 게이트가 있는 스코어카드.
//!
//! `shadow_cases.rs`의 `report_*` 는 사람이 읽는 진단 출력이고 통과/실패를 판정하지 않는다.
//! 이 파일은 같은 케이스 카탈로그로 **수치를 뽑고, 기준선과 비교해 회귀하면 실패한다.**
//! 규칙 엔진을 건드릴 때마다 "좋아진 것 같다"가 아니라 숫자로 확인하기 위한 장치다.
//!
//! ```text
//! cargo test --test eval_scorecard -- --nocapture     # 실행 + 게이트
//! UPDATE_EVAL_BASELINE=1 cargo test --test eval_scorecard   # 기준선 갱신
//! ```
//!
//! 산출물:
//!   tests/eval/scorecard.json — 기계 판독용 (기준선 비교 대상)
//!   tests/eval/scorecard.md   — 사람이 읽는 요약
//!   tests/eval/baseline.json  — 커밋된 기준선

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use common::*;
use style_engine::models::outfit::{OutfitContext, SlotKind};
use style_engine::services::style_engine_v2::{self, TodayFitLevel};

/// 지표가 이 값(%p)보다 더 떨어지면 회귀로 본다.
/// 0으로 두면 케이스를 한 건 추가하는 것만으로도 실패하므로 약간의 여유를 둔다.
const REGRESSION_TOLERANCE_PCT: f64 = 1.0;

// ─── 스코어카드 스키마 ───

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Scorecard {
    total_cases: usize,
    hard_filter: HardFilterMetrics,
    today_fit: TodayFitMetrics,
    preference: PreferenceMetrics,
    combined_accuracy_pct: f64,
    /// 사람이 두 코디 중 고른 쪽을 엔진도 높게 매기는가 (`preference_pairs.toml`).
    #[serde(default)]
    human_pairs: HumanPairMetrics,
    /// 홈 추천에서 사용자 반응으로 정해진 쌍 (`usage_pairs.toml`).
    #[serde(default)]
    usage_pairs: UsagePairMetrics,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct HumanPairMetrics {
    pairs: usize,
    /// 운영 엔진(style_engine) 점수로 사람이 고른 쪽이 더 높은 비율(동점 0.5).
    production_agree_pct: f64,
    /// shadow 엔진(style_engine_v2 스타일 점수 + 상황 보정)으로 같은 비율.
    shadow_agree_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct UsagePairMetrics {
    pairs: usize,
    /// 지운 코디를 진 쪽으로 둔 쌍. 나머지는 반응 없는 코디가 진 쪽이다.
    strong_pairs: usize,
    production_agree_pct: f64,
    shadow_agree_pct: f64,
    /// 화면에 먼저 나온 코디가 이긴 비율. 엔진 점수가 이보다 낫지 않으면 순서를 바꿀
    /// 근거가 없다는 뜻이라 비교용으로 둔다.
    screen_order_agree_pct: f64,
}

#[derive(Debug, Deserialize)]
struct PairFile {
    pairs: Vec<HumanPair>,
}

#[derive(Debug, Deserialize)]
struct HumanPair {
    #[allow(dead_code)]
    id: String,
    situation: Option<String>,
    #[serde(default)]
    strength: Option<String>,
    /// [이긴 쪽, 진 쪽] 의 화면 순서(0부터). `usage_pairs.toml` 에만 있다.
    #[serde(default)]
    positions: Option<[u8; 2]>,
    better: PairOutfit,
    worse: PairOutfit,
}

#[derive(Debug, Deserialize)]
struct PairOutfit {
    #[serde(default)]
    top: String,
    #[serde(default)]
    bottom: String,
    #[serde(default)]
    outer: String,
    #[serde(default)]
    shoes: String,
    #[serde(default)]
    bag: String,
}

fn pair_context(o: &PairOutfit, situation: &Option<String>, registry: &Registry) -> OutfitContext {
    let slots = [
        (&o.top, SlotKind::Top),
        (&o.bottom, SlotKind::Bottom),
        (&o.outer, SlotKind::Outer),
        (&o.shoes, SlotKind::Shoes),
        (&o.bag, SlotKind::Bag),
    ]
    .iter()
    .filter_map(|(id, kind)| registry_to_outfit_slot(id, *kind, registry))
    .collect();
    OutfitContext {
        slots,
        situation: situation.clone(),
    }
}

/// 사람이 고른 쌍에서 두 엔진이 같은 쪽을 높게 매기는 비율.
///
/// 블라인드 비교에서 사람은 운영 9 : shadow 6 으로 골랐는데, 위의 라벨 지표로는 shadow
/// 엔진이 더 좋아 보였다. 라벨이 실제 취향을 다 담지 못한다는 뜻이라, 취향 판단을 직접
/// 재는 이 지표를 따로 둔다.
fn compute_human_pairs(registry: &Registry) -> HumanPairMetrics {
    let file = load_pairs("preference_pairs.toml");
    let (prod, shadow) = engine_agreement(&file.pairs, registry);
    let n = file.pairs.len();
    HumanPairMetrics {
        pairs: n,
        production_agree_pct: round1(100.0 * prod / n as f64),
        shadow_agree_pct: round1(100.0 * shadow / n as f64),
    }
}

/// 홈 추천에서 반응으로 정해진 쌍. 블라인드 비교와 달리 운영 엔진이 이미 고른 후보끼리라,
/// 운영 엔진이 걸러 낸 뒤에도 남는 취향 차이를 엔진 점수가 따라가는지 본다.
fn compute_usage_pairs(registry: &Registry) -> UsagePairMetrics {
    let file = load_pairs("usage_pairs.toml");
    let (prod, shadow) = engine_agreement(&file.pairs, registry);
    let n = file.pairs.len();
    let order: f64 = file
        .pairs
        .iter()
        .map(|p| match p.positions {
            Some([b, w]) if b < w => 1.0,
            Some([b, w]) if b == w => 0.5,
            _ => 0.0,
        })
        .sum();
    UsagePairMetrics {
        pairs: n,
        strong_pairs: file
            .pairs
            .iter()
            .filter(|p| p.strength.as_deref() == Some("strong"))
            .count(),
        production_agree_pct: round1(100.0 * prod / n as f64),
        shadow_agree_pct: round1(100.0 * shadow / n as f64),
        screen_order_agree_pct: round1(100.0 * order / n as f64),
    }
}

fn load_pairs(name: &str) -> PairFile {
    let raw = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|_| panic!("{name} 을 읽지 못했다"));
    toml::from_str(&raw).unwrap_or_else(|e| panic!("{name} 파싱 실패: {e}"))
}

/// 두 엔진이 이긴 쪽을 더 높게 매긴 쌍의 수(동점 0.5) — (운영, shadow).
fn engine_agreement(pairs: &[HumanPair], registry: &Registry) -> (f64, f64) {
    let (mut prod, mut shadow) = (0.0, 0.0);
    let agree = |better: i32, worse: i32| match better.cmp(&worse) {
        std::cmp::Ordering::Greater => 1.0,
        std::cmp::Ordering::Equal => 0.5,
        std::cmp::Ordering::Less => 0.0,
    };
    let shadow_score = |ctx: &OutfitContext| {
        let sub = style_engine_v2::compute_subscores(ctx, None);
        style_engine_v2::compute_style_score(&sub)
            + style_engine::services::serving_ranker::compute_serving_adjustment(ctx).0
    };
    for p in pairs {
        let b = pair_context(&p.better, &p.situation, registry);
        let w = pair_context(&p.worse, &p.situation, registry);
        prod += agree(
            style_engine::services::style_engine::evaluate(&b, None).score,
            style_engine::services::style_engine::evaluate(&w, None).score,
        );
        shadow += agree(shadow_score(&b), shadow_score(&w));
    }
    (prod, shadow)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct HardFilterMetrics {
    accuracy_pct: f64,
    /// 엔진이 거절했지만 사람은 수용 — 사용자에게 가장 아픈 오류.
    false_positives: usize,
    /// 엔진이 통과시켰지만 사람은 거절.
    false_negatives: usize,
    /// "거절"을 양성으로 본 정밀도/재현율.
    reject_precision_pct: f64,
    reject_recall_pct: f64,
    /// false positive를 유발한 룰별 건수.
    fp_by_reason: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TodayFitMetrics {
    accuracy_pct: f64,
    /// "expected→actual" → 건수.
    confusion: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PreferenceMetrics {
    /// Accept 케이스가 Reject 케이스보다 높은 점수를 받은 쌍의 비율.
    /// `temperature` 태그가 붙은 케이스는 넣지 않는다(스타일 점수는 기온을 보지 않는다).
    /// 임계값을 정하지 않아도 되므로 점수 스케일이 바뀌어도 비교 가능하다.
    pairwise_ranking_pct: f64,
    accept_mean_score: f64,
    reject_mean_score: f64,
    /// 두 그룹 평균의 간격. 클수록 엔진이 두 집단을 잘 분리한다.
    separation: f64,
}

// ─── 계산 ───

fn fit_label(f: &TodayFitLevel) -> &'static str {
    match f {
        TodayFitLevel::Pass => "Pass",
        TodayFitLevel::Borderline => "Borderline",
        TodayFitLevel::Fail => "Fail",
    }
}

fn pct(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    round1(100.0 * numerator as f64 / denominator as f64)
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn compute_scorecard() -> Scorecard {
    let registry = load_registry();
    let cases = load_cases().cases;

    let mut hard_tp = 0; // 엔진 거절 = 사람 거절
    let mut hard_tn = 0; // 엔진 통과 = 사람 통과
    let mut fp = 0;
    let mut fn_ = 0;
    let mut fp_by_reason: BTreeMap<String, usize> = BTreeMap::new();

    let mut fit_agree = 0;
    let mut confusion: BTreeMap<String, usize> = BTreeMap::new();

    let mut combined_agree = 0;

    let mut accept_scores: Vec<i32> = Vec::new();
    let mut reject_scores: Vec<i32> = Vec::new();

    for case in &cases {
        let ctx = case_to_context(case, &registry);
        let season = case.current_season.as_deref();
        let temp = case.temperature_c.unwrap_or(20.0);

        let hard = style_engine_v2::run_hard_filter(&ctx, season);
        let sub = style_engine_v2::compute_subscores(&ctx, season);
        let score = style_engine_v2::compute_style_score(&sub);
        let fit = style_engine::services::serving_ranker::compute_today_fit(&ctx, temp);

        // hard filter
        match (hard.pass, case.expected_hard_pass) {
            (true, true) => hard_tn += 1,
            (false, false) => hard_tp += 1,
            (false, true) => {
                fp += 1;
                for r in &hard.reasons {
                    *fp_by_reason.entry(reason_code(r).to_string()).or_insert(0) += 1;
                }
            }
            (true, false) => fn_ += 1,
        }

        // today fit
        let actual_fit = fit_label(&fit);
        if actual_fit == case.expected_today_fit {
            fit_agree += 1;
        } else {
            *confusion
                .entry(format!("{}→{}", case.expected_today_fit, actual_fit))
                .or_insert(0) += 1;
        }

        if hard.pass == case.expected_hard_pass && actual_fit == case.expected_today_fit {
            combined_agree += 1;
        }

        // preference — 점수 분리도
        //
        // 기온 때문에 거절한 케이스는 뺀다. 스타일 점수는 기온을 보지 않으므로, 10도에
        // 반팔 단독(TG001)처럼 옷차림 자체는 멀쩡한 조합이 높은 점수를 받는 것은 엔진의
        // 실수가 아니다. 넣어 두면 기온 라벨을 하나 추가할 때마다 이 지표가 떨어진다
        // (TG011 추가로 75.1 → 73.8%). 기온 판단은 위의 today-fit 지표가 잰다.
        if case.tags.iter().any(|t| t == "temperature") {
            continue;
        }
        match case.expected_preference.as_str() {
            "Accept" => accept_scores.push(score),
            "Reject" => reject_scores.push(score),
            _ => {} // Borderline은 랭킹 비교에서 제외
        }
    }

    let total = cases.len();

    // Accept가 Reject보다 높게 매겨진 쌍의 비율 (동점은 0.5점).
    let mut correct_pairs = 0.0;
    let mut total_pairs = 0.0;
    for a in &accept_scores {
        for r in &reject_scores {
            total_pairs += 1.0;
            if a > r {
                correct_pairs += 1.0;
            } else if a == r {
                correct_pairs += 0.5;
            }
        }
    }

    let mean = |v: &[i32]| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            round1(v.iter().sum::<i32>() as f64 / v.len() as f64)
        }
    };
    let accept_mean = mean(&accept_scores);
    let reject_mean = mean(&reject_scores);

    Scorecard {
        total_cases: total,
        hard_filter: HardFilterMetrics {
            accuracy_pct: pct(hard_tp + hard_tn, total),
            false_positives: fp,
            false_negatives: fn_,
            reject_precision_pct: pct(hard_tp, hard_tp + fp),
            reject_recall_pct: pct(hard_tp, hard_tp + fn_),
            fp_by_reason,
        },
        today_fit: TodayFitMetrics {
            accuracy_pct: pct(fit_agree, total),
            confusion,
        },
        preference: PreferenceMetrics {
            pairwise_ranking_pct: if total_pairs == 0.0 {
                0.0
            } else {
                round1(100.0 * correct_pairs / total_pairs)
            },
            accept_mean_score: accept_mean,
            reject_mean_score: reject_mean,
            separation: round1(accept_mean - reject_mean),
        },
        combined_accuracy_pct: pct(combined_agree, total),
        human_pairs: compute_human_pairs(&registry),
        usage_pairs: compute_usage_pairs(&registry),
    }
}

// ─── 산출물 ───

fn eval_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/eval")
        .join(name)
}

fn render_markdown(s: &Scorecard) -> String {
    let mut out = String::new();
    out.push_str("# Style Engine Eval Scorecard\n\n");
    out.push_str(&format!(
        "케이스 {}건 (`tests/fixtures/recommendation_cases.toml`)\n\n",
        s.total_cases
    ));

    out.push_str("| 지표 | 값 |\n|---|---|\n");
    out.push_str(&format!(
        "| Hard filter 정확도 | **{:.1}%** |\n",
        s.hard_filter.accuracy_pct
    ));
    out.push_str(&format!(
        "| — false positive (엔진 거절 / 사람 수용) | {} |\n",
        s.hard_filter.false_positives
    ));
    out.push_str(&format!(
        "| — false negative (엔진 통과 / 사람 거절) | {} |\n",
        s.hard_filter.false_negatives
    ));
    out.push_str(&format!(
        "| — 거절 정밀도 / 재현율 | {:.1}% / {:.1}% |\n",
        s.hard_filter.reject_precision_pct, s.hard_filter.reject_recall_pct
    ));
    out.push_str(&format!(
        "| Today-fit 정확도 (3-class) | **{:.1}%** |\n",
        s.today_fit.accuracy_pct
    ));
    out.push_str(&format!(
        "| 선호도 순위 정확도 (Accept > Reject 쌍) | **{:.1}%** |\n",
        s.preference.pairwise_ranking_pct
    ));
    out.push_str(&format!(
        "| — Accept 평균 / Reject 평균 / 간격 | {:.1} / {:.1} / {:.1} |\n",
        s.preference.accept_mean_score, s.preference.reject_mean_score, s.preference.separation
    ));
    out.push_str(&format!(
        "| Hard + fit 동시 일치 | **{:.1}%** |\n",
        s.combined_accuracy_pct
    ));
    out.push_str(&format!(
        "| 사람이 고른 쌍과 일치 — 운영 / shadow ({}쌍, `preference_pairs.toml`) | **{:.1}%** / **{:.1}%** |\n",
        s.human_pairs.pairs, s.human_pairs.production_agree_pct, s.human_pairs.shadow_agree_pct
    ));
    out.push_str(&format!(
        "| 반응으로 정해진 쌍과 일치 — 운영 / shadow / 화면 순서 ({}쌍 · 지운 코디 {}쌍, `usage_pairs.toml`) | **{:.1}%** / **{:.1}%** / {:.1}% |\n\n",
        s.usage_pairs.pairs,
        s.usage_pairs.strong_pairs,
        s.usage_pairs.production_agree_pct,
        s.usage_pairs.shadow_agree_pct,
        s.usage_pairs.screen_order_agree_pct
    ));

    if !s.hard_filter.fp_by_reason.is_empty() {
        out.push_str("## False positive를 유발한 룰\n\n| 룰 | 건수 |\n|---|---|\n");
        let mut rows: Vec<_> = s.hard_filter.fp_by_reason.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (reason, count) in rows {
            out.push_str(&format!("| `{reason}` | {count} |\n"));
        }
        out.push('\n');
    }

    if !s.today_fit.confusion.is_empty() {
        out.push_str("## Today-fit 오분류 (expected→actual)\n\n| 전이 | 건수 |\n|---|---|\n");
        let mut rows: Vec<_> = s.today_fit.confusion.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (k, v) in rows {
            out.push_str(&format!("| {k} | {v} |\n"));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "> `cargo test --test eval_scorecard`로 재생성됩니다. \
         기준선 대비 {REGRESSION_TOLERANCE_PCT:.1}%p 이상 하락하면 테스트가 실패합니다.\n"
    ));
    out
}

// ─── 게이트 ───

/// 기준선 대비 하락한 지표를 모아 돌려준다.
fn regressions(baseline: &Scorecard, current: &Scorecard) -> Vec<String> {
    let checks: [(&str, f64, f64); 9] = [
        (
            "usage_pairs.production_agree_pct",
            baseline.usage_pairs.production_agree_pct,
            current.usage_pairs.production_agree_pct,
        ),
        (
            "usage_pairs.shadow_agree_pct",
            baseline.usage_pairs.shadow_agree_pct,
            current.usage_pairs.shadow_agree_pct,
        ),
        (
            "human_pairs.production_agree_pct",
            baseline.human_pairs.production_agree_pct,
            current.human_pairs.production_agree_pct,
        ),
        (
            "human_pairs.shadow_agree_pct",
            baseline.human_pairs.shadow_agree_pct,
            current.human_pairs.shadow_agree_pct,
        ),
        (
            "hard_filter.accuracy_pct",
            baseline.hard_filter.accuracy_pct,
            current.hard_filter.accuracy_pct,
        ),
        (
            "hard_filter.reject_precision_pct",
            baseline.hard_filter.reject_precision_pct,
            current.hard_filter.reject_precision_pct,
        ),
        (
            "today_fit.accuracy_pct",
            baseline.today_fit.accuracy_pct,
            current.today_fit.accuracy_pct,
        ),
        (
            "preference.pairwise_ranking_pct",
            baseline.preference.pairwise_ranking_pct,
            current.preference.pairwise_ranking_pct,
        ),
        (
            "combined_accuracy_pct",
            baseline.combined_accuracy_pct,
            current.combined_accuracy_pct,
        ),
    ];

    let mut out = Vec::new();
    for (name, base, cur) in checks {
        if cur < base - REGRESSION_TOLERANCE_PCT {
            out.push(format!(
                "{name}: {base:.1}% → {cur:.1}% ({:+.1}%p)",
                cur - base
            ));
        }
    }

    // false positive 증가는 비율이 유지돼도 사용자에게는 악화다.
    if current.hard_filter.false_positives > baseline.hard_filter.false_positives {
        out.push(format!(
            "hard_filter.false_positives: {} → {} (증가)",
            baseline.hard_filter.false_positives, current.hard_filter.false_positives
        ));
    }
    out
}

#[test]
fn eval_scorecard_has_no_regression() {
    let current = compute_scorecard();

    let json = serde_json::to_string_pretty(&current).expect("scorecard 직렬화 실패");
    std::fs::write(eval_path("scorecard.json"), format!("{json}\n"))
        .expect("scorecard.json 쓰기 실패");
    std::fs::write(eval_path("scorecard.md"), render_markdown(&current))
        .expect("scorecard.md 쓰기 실패");

    println!("\n{}", render_markdown(&current));

    let baseline_path = eval_path("baseline.json");

    if std::env::var("UPDATE_EVAL_BASELINE").is_ok() {
        std::fs::write(&baseline_path, format!("{json}\n")).expect("baseline 쓰기 실패");
        println!("기준선을 갱신했습니다: {}", baseline_path.display());
        return;
    }

    let Ok(raw) = std::fs::read_to_string(&baseline_path) else {
        panic!(
            "기준선이 없습니다. 먼저 `UPDATE_EVAL_BASELINE=1 cargo test --test eval_scorecard`를 실행하세요."
        );
    };
    let baseline: Scorecard = serde_json::from_str(&raw).expect("baseline.json 파싱 실패");

    let regressions = regressions(&baseline, &current);
    assert!(
        regressions.is_empty(),
        "\n엔진 품질이 기준선 대비 하락했습니다:\n  {}\n\n\
         의도한 변경이라면: UPDATE_EVAL_BASELINE=1 cargo test --test eval_scorecard\n",
        regressions.join("\n  ")
    );
}

/// 입력이 완전히 같은데 정답이 다른 케이스가 있으면 어떤 엔진도 둘 다 맞출 수 없다.
///
/// 이런 쌍은 달성 가능한 정확도에 상한을 씌우므로, 지표를 튜닝하기 전에 없애야 한다.
/// 실제로 이 검사를 처음 넣었을 때 두 쌍이 걸렸고, 둘 다 fixture 작성 실수였다.
#[test]
fn no_contradictory_cases() {
    let cases = load_cases().cases;
    let mut by_input: BTreeMap<String, Vec<&TestCase>> = BTreeMap::new();
    for c in &cases {
        let key = format!(
            "{}|{}|{}|{}",
            c.situation.as_deref().unwrap_or(""),
            c.temperature_c.unwrap_or(20.0),
            c.current_season.as_deref().unwrap_or(""),
            build_outfit_key(c),
        );
        by_input.entry(key).or_default().push(c);
    }

    let mut conflicts = Vec::new();
    for (_, group) in by_input {
        if group.len() < 2 {
            continue;
        }
        let first = group[0];
        for other in &group[1..] {
            if other.expected_today_fit != first.expected_today_fit
                || other.expected_hard_pass != first.expected_hard_pass
                || other.expected_preference != first.expected_preference
            {
                conflicts.push(format!(
                    "{} vs {}: fit {}/{}, hard {}/{}, pref {}/{}",
                    first.case_id,
                    other.case_id,
                    first.expected_today_fit,
                    other.expected_today_fit,
                    first.expected_hard_pass,
                    other.expected_hard_pass,
                    first.expected_preference,
                    other.expected_preference,
                ));
            }
        }
    }

    assert!(
        conflicts.is_empty(),
        "\n입력이 같은데 정답이 다른 케이스가 있습니다. 어떤 엔진도 둘 다 맞출 수 없습니다:\n  {}\n",
        conflicts.join("\n  ")
    );
}

/// 케이스 카탈로그가 지표를 의미 있게 만들 만큼 확보돼 있는지.
#[test]
fn scorecard_covers_both_classes() {
    let s = compute_scorecard();
    assert!(
        s.total_cases >= 50,
        "케이스가 너무 적습니다: {}",
        s.total_cases
    );
    assert!(
        s.preference.accept_mean_score > 0.0 && s.preference.reject_mean_score > 0.0,
        "Accept/Reject 양쪽 케이스가 모두 있어야 순위 지표가 의미를 가집니다"
    );
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    fn card(hard_acc: f64, fp: usize) -> Scorecard {
        Scorecard {
            total_cases: 96,
            hard_filter: HardFilterMetrics {
                accuracy_pct: hard_acc,
                false_positives: fp,
                false_negatives: 0,
                reject_precision_pct: 90.0,
                reject_recall_pct: 90.0,
                fp_by_reason: BTreeMap::new(),
            },
            today_fit: TodayFitMetrics {
                accuracy_pct: 80.0,
                confusion: BTreeMap::new(),
            },
            preference: PreferenceMetrics {
                pairwise_ranking_pct: 70.0,
                accept_mean_score: 70.0,
                reject_mean_score: 50.0,
                separation: 20.0,
            },
            combined_accuracy_pct: 75.0,
            human_pairs: HumanPairMetrics::default(),
            usage_pairs: UsagePairMetrics::default(),
        }
    }

    #[test]
    fn small_fluctuation_within_tolerance_is_not_a_regression() {
        let base = card(80.0, 3);
        let cur = card(79.5, 3);
        assert!(regressions(&base, &cur).is_empty());
    }

    #[test]
    fn drop_beyond_tolerance_is_flagged() {
        let base = card(80.0, 3);
        let cur = card(75.0, 3);
        assert_eq!(regressions(&base, &cur).len(), 1);
    }

    #[test]
    fn new_false_positive_is_flagged_even_when_accuracy_holds() {
        let base = card(80.0, 3);
        let cur = card(80.0, 4);
        assert_eq!(regressions(&base, &cur).len(), 1);
    }

    #[test]
    fn improvement_is_never_a_regression() {
        let base = card(80.0, 3);
        let cur = card(95.0, 0);
        assert!(regressions(&base, &cur).is_empty());
    }
}
