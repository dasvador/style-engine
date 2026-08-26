//! Recommendation Experiment — shadow mode 로그 수집기 (S4 확장).
//!
//! baseline 추천 경로와 병행 실행되며 오직 로그만 남긴다.
//! S4에서 추가: today_fit 실제 계산, serving adjustment, experiment mode winners.

use serde::Serialize;
use sqlx::MySqlPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::models::clothing::Clothing;
use crate::models::recommendation::OutfitCandidate;
use crate::services::candidate_pipeline;
use crate::services::serving_ranker;
use crate::services::style_engine_v2::{HardFilterReason, SubScores, TodayFitLevel};

#[derive(Debug, Clone, Serialize)]
pub struct CandidateShadowLog {
    pub ai_candidate_index: usize,
    pub baseline_style_score: i32,
    pub experiment_style_score: i32,
    pub hard_pass: bool,
    pub hard_fail_reasons: Vec<HardFilterReason>,
    pub today_fit: TodayFitLevel,
    pub sub_scores: SubScores,
    /// serving adjustment (situation-aware accessory 보정)
    pub serving_adjustment: i32,
    /// style_score + serving_adjustment
    pub serving_score: i32,
    pub serving_reason: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
// DormantTiebreak / ContextPenaltyDiff 는 shadow 로그 스키마에 예약된 분류로,
// 해당 판정 분기가 아직 구현되지 않아 생성되지 않는다.
#[allow(dead_code)]
pub enum WinnerChangeReason {
    HardFilterDiff,
    TodayFitDiff,
    StyleScoreDiff,
    AccessoryPenaltyDiff,
    RecencyTiebreak,
    DiversityTiebreak,
    DormantTiebreak,
    QualificationGateDiff,
    ContextPenaltyDiff,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct WinnerChangeAnalysis {
    pub changed: bool,
    pub primary_reason: WinnerChangeReason,
    pub secondary_reasons: Vec<WinnerChangeReason>,
    pub baseline_idx: Option<usize>,
    pub experiment_idx: Option<usize>,
    pub baseline_style_score: Option<i32>,
    pub experiment_style_score: Option<i32>,
    pub baseline_serving_score: Option<i32>,
    pub experiment_serving_score: Option<i32>,
    pub baseline_today_fit: Option<TodayFitLevel>,
    pub experiment_today_fit: Option<TodayFitLevel>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendationExperimentLog {
    pub request_id: String,
    // winner 비교 (S5)
    pub winner: WinnerChangeAnalysis,
    // baseline winners
    pub baseline_variation_idx: Option<usize>,
    pub baseline_dormant_idx: Option<usize>,
    // experiment winners
    pub experiment_variation_idx: Option<usize>,
    pub experiment_dormant_idx: Option<usize>,
    // 상세
    pub experiment_survivor_indices: Vec<usize>,
    pub candidates: Vec<CandidateShadowLog>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_shadow(
    db: &MySqlPool,
    reranked: &[OutfitCandidate],
    clothes: &[Clothing],
    occasion: Option<&str>,
    current_season: Option<&str>,
    temperature: f64,
    baseline_today_idx: Option<usize>,
    baseline_variation_idx: Option<usize>,
    baseline_dormant_idx: Option<usize>,
    dormant_ids: &std::collections::HashSet<String>,
) {
    let request_id = Uuid::new_v4().to_string();
    let mut cands = Vec::new();
    let mut survivors = Vec::new();

    for c in reranked {
        let ctx = match candidate_pipeline::rebuild_context(c, clothes, db, occasion).await {
            Some(x) => x,
            None => continue,
        };
        let items: Vec<String> = ctx
            .slots
            .iter()
            .map(|s| {
                format!(
                    "{}:{}(style={},role={})",
                    s.slot.label(),
                    s.clothing.name,
                    s.clothing.style.map(|v| v.as_str()).unwrap_or("-"),
                    s.clothing.role.map(|v| v.as_str()).unwrap_or("-"),
                )
            })
            .collect();
        let eval = candidate_pipeline::evaluate_v2(&ctx, current_season, temperature);
        let (adj, reason) = serving_ranker::compute_serving_adjustment(&ctx);
        let serving_score = eval.style_score + adj;

        if eval.hard.pass {
            survivors.push(c.ai_candidate_index);
        }
        cands.push(CandidateShadowLog {
            ai_candidate_index: c.ai_candidate_index,
            baseline_style_score: c.style_score,
            experiment_style_score: eval.style_score,
            hard_pass: eval.hard.pass,
            hard_fail_reasons: eval.hard.reasons,
            today_fit: eval.today_fit,
            sub_scores: eval.sub,
            serving_adjustment: adj,
            serving_score,
            serving_reason: reason,
            items,
        });
    }

    // ─── Experiment mode winners (serving ranker 순서) ───
    // qualified: hard_pass + today_fit != Fail
    let mut qualified: Vec<(usize, &CandidateShadowLog, &OutfitCandidate)> = cands
        .iter()
        .enumerate()
        .filter(|(_, c)| c.hard_pass && c.today_fit != TodayFitLevel::Fail)
        .filter_map(|(i, c)| {
            reranked
                .iter()
                .find(|r| r.ai_candidate_index == c.ai_candidate_index)
                .map(|r| (i, c, r))
        })
        .collect();

    qualified.sort_by_key(|(_, c, r)| {
        serving_ranker::serving_sort_key(
            c.hard_pass,
            c.today_fit,
            c.serving_score,
            r.recency_penalty,
            r.diversity_bonus,
            c.ai_candidate_index,
        )
    });

    let exp_today_idx = qualified.first().map(|(_, c, _)| c.ai_candidate_index);

    let exp_variation_idx = exp_today_idx.and_then(|today| {
        qualified
            .iter()
            .find(|(_, c, r)| {
                c.ai_candidate_index != today
                    && (r.top_id
                        != reranked
                            .iter()
                            .find(|x| x.ai_candidate_index == today)
                            .and_then(|t| t.top_id.clone())
                        || r.bottom_id
                            != reranked
                                .iter()
                                .find(|x| x.ai_candidate_index == today)
                                .and_then(|t| t.bottom_id.clone()))
            })
            .map(|(_, c, _)| c.ai_candidate_index)
    });

    let exp_dormant_idx = exp_today_idx.and_then(|today| {
        qualified
            .iter()
            .find(|(_, c, r)| {
                c.ai_candidate_index != today
                    && Some(c.ai_candidate_index) != exp_variation_idx
                    && crate::services::recommendation_service::contains_dormant_item(
                        r,
                        dormant_ids,
                    )
            })
            .map(|(_, c, _)| c.ai_candidate_index)
    });

    // ─── S5: winner 비교 ───
    let winner_changed = baseline_today_idx != exp_today_idx;

    let b_cand =
        baseline_today_idx.and_then(|idx| cands.iter().find(|c| c.ai_candidate_index == idx));
    let e_cand = exp_today_idx.and_then(|idx| cands.iter().find(|c| c.ai_candidate_index == idx));
    let b_oc =
        baseline_today_idx.and_then(|idx| reranked.iter().find(|r| r.ai_candidate_index == idx));

    let (primary, secondaries, note) = if !winner_changed {
        (
            WinnerChangeReason::Unknown,
            vec![],
            "same winner".to_string(),
        )
    } else {
        determine_change_reasons(b_cand, e_cand, b_oc, reranked)
    };

    let winner = WinnerChangeAnalysis {
        changed: winner_changed,
        primary_reason: primary,
        secondary_reasons: secondaries,
        baseline_idx: baseline_today_idx,
        experiment_idx: exp_today_idx,
        baseline_style_score: b_oc.map(|r| r.style_score),
        experiment_style_score: e_cand.map(|c| c.experiment_style_score),
        baseline_serving_score: b_cand.map(|c| c.serving_score),
        experiment_serving_score: e_cand.map(|c| c.serving_score),
        baseline_today_fit: b_cand.map(|c| c.today_fit),
        experiment_today_fit: e_cand.map(|c| c.today_fit),
        note,
    };

    let log = RecommendationExperimentLog {
        request_id,
        winner,
        baseline_variation_idx,
        baseline_dormant_idx,
        experiment_variation_idx: exp_variation_idx,
        experiment_dormant_idx: exp_dormant_idx,
        experiment_survivor_indices: survivors,
        candidates: cands,
    };

    match serde_json::to_string(&log) {
        Ok(json) => info!("shadow_log={}", json),
        Err(e) => warn!("failed to serialize shadow log: {e}"),
    }
}

/// baseline winner와 experiment winner가 달라진 이유를 우선순위로 판별.
/// primary + secondary reasons 구조로 반환.
fn determine_change_reasons(
    b_cand: Option<&CandidateShadowLog>,
    e_cand: Option<&CandidateShadowLog>,
    b_oc: Option<&OutfitCandidate>,
    reranked: &[OutfitCandidate],
) -> (WinnerChangeReason, Vec<WinnerChangeReason>, String) {
    let mut reasons = Vec::new();
    let mut notes = Vec::new();

    // 1. baseline winner가 v2 hard filter에서 탈락
    if let Some(bc) = b_cand
        && !bc.hard_pass
    {
        reasons.push(WinnerChangeReason::HardFilterDiff);
        notes.push(format!(
            "baseline idx={} hard-failed in v2",
            bc.ai_candidate_index
        ));
    }

    // 2. baseline winner의 today_fit이 Fail
    if let Some(bc) = b_cand {
        if bc.today_fit == TodayFitLevel::Fail {
            reasons.push(WinnerChangeReason::TodayFitDiff);
            notes.push("baseline winner today_fit=Fail".to_string());
        } else if bc.today_fit == TodayFitLevel::Borderline
            && e_cand.is_some_and(|ec| ec.today_fit == TodayFitLevel::Pass)
        {
            reasons.push(WinnerChangeReason::QualificationGateDiff);
            notes.push("baseline Borderline vs experiment Pass".to_string());
        }
    }

    // 3. style_score 차이
    if let (Some(bc), Some(ec)) = (b_cand, e_cand)
        && bc.experiment_style_score < ec.experiment_style_score
    {
        reasons.push(WinnerChangeReason::StyleScoreDiff);
        notes.push(format!(
            "v2 style: baseline={} < experiment={}",
            bc.experiment_style_score, ec.experiment_style_score
        ));
    }

    // 4. serving adjustment 차이
    if let (Some(bc), Some(ec)) = (b_cand, e_cand)
        && bc.serving_adjustment != ec.serving_adjustment
        && bc.serving_score < ec.serving_score
    {
        reasons.push(WinnerChangeReason::AccessoryPenaltyDiff);
        notes.push(format!(
            "serving adj: baseline={} vs experiment={}",
            bc.serving_adjustment, ec.serving_adjustment
        ));
    }

    // 5. recency/diversity tie-break
    if let Some(ec) = e_cand {
        let e_oc = reranked
            .iter()
            .find(|r| r.ai_candidate_index == ec.ai_candidate_index);
        if let (Some(bo), Some(eo)) = (b_oc, e_oc) {
            if bo.recency_penalty > eo.recency_penalty {
                reasons.push(WinnerChangeReason::RecencyTiebreak);
            }
            if bo.diversity_bonus < eo.diversity_bonus {
                reasons.push(WinnerChangeReason::DiversityTiebreak);
            }
        }
    }

    if reasons.is_empty() {
        reasons.push(WinnerChangeReason::Unknown);
    }

    let primary = reasons.remove(0);
    let note = notes.join("; ");
    (primary, reasons, note)
}
