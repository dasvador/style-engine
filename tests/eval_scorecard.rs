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
        "| Hard + fit 동시 일치 | **{:.1}%** |\n\n",
        s.combined_accuracy_pct
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
    let checks: [(&str, f64, f64); 5] = [
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
