//! 고정 후보 재생: 같은 후보·날씨·이력·피드백에서 변경 전후 선택이 왜 달라지는지 본다.
//!
//! 후보는 실제 LLM 이 만든 것을 파일로 받는다(`[{temp, candidates: [[이름…]…]}]`). 새
//! 프롬프트의 효과는 여기서 주장하지 않는다 — 후보가 고정이기 때문이다. 채점·게이트·반복
//! 보정·카드 선택만 비교한다. 계절은 '가을' 로 고정한다. DB 는 읽기만 한다.
//!
//! "변경 전" 경로는 이 파일 안에 옮겨 둔 예전 규칙이다(`legacy_*`): 무드 없는 평가, 아무
//! 슬롯이나 글램이면 통과하는 게이트, 신발·가방도 옷과 같이 세는 반복 감점, 날씨 실패를
//! 되돌려 넣는 게이트, Today 만 피하는 '안 입은 옷'.
//!
//!     cargo run --example glam_replay -- <후보.json> <user_id>
use std::collections::{HashMap, HashSet};

use sqlx::mysql::MySqlPoolOptions;
use style_engine::db::clothing_repo;
use style_engine::db::recommendation_history_repo::RecommendationHistoryRepo;
use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::recommendation::OutfitCandidate;
use style_engine::models::recommendation_history::{
    OutfitHistorySummary, OutfitRecommendationHistory,
};
use style_engine::models::style_vocab::StyleGenre;
use style_engine::services::recommendation_diversity::{
    calculate_diversity_bonus, calculate_recency_penalty,
};
use style_engine::services::recommendation_service as svc;
use style_engine::services::serving_ranker::compute_today_fit;
use style_engine::services::style_engine as engine;
use style_engine::services::style_engine_v2::TodayFitLevel;
use style_engine::services::{mood_gate, mood_rules, outfit_scorer, unfit_gate};

const SEASON: &str = "가을";

fn kind(c: &str) -> SlotKind {
    match c {
        "상의" => SlotKind::Top,
        "하의" => SlotKind::Bottom,
        "아우터" => SlotKind::Outer,
        "신발" => SlotKind::Shoes,
        _ => SlotKind::Bag,
    }
}

/// 예전 반복 요약 — 다섯 칸 중 하나만 겹쳐도 "아이템 재사용" 이었다.
fn legacy_summary(
    c: &OutfitCandidate,
    rows: &[OutfitRecommendationHistory],
) -> OutfitHistorySummary {
    let mut s = OutfitHistorySummary::default();
    let now = chrono::Local::now().naive_local();
    for row in rows {
        let days = (now - row.recommended_at).num_days();
        let same_item = [
            (&c.top_id, &row.top_id),
            (&c.bottom_id, &row.bottom_id),
            (&c.outer_id, &row.outer_id),
            (&c.shoes_id, &row.shoes_id),
            (&c.bag_id, &row.bag_id),
        ]
        .iter()
        .any(|(a, b)| a.is_some() && a == b);
        if same_item && days <= 1 {
            s.item_used_yesterday = true;
        }
        if same_item && days <= 3 {
            s.item_used_within_3_days = true;
        }
        if days <= 7 && c.top_id == row.top_id && c.bottom_id == row.bottom_id {
            s.same_top_bottom_within_7_days = true;
        }
        if days <= 14
            && c.top_id == row.top_id
            && c.bottom_id == row.bottom_id
            && c.outer_id == row.outer_id
            && c.shoes_id == row.shoes_id
            && c.bag_id == row.bag_id
        {
            s.same_full_outfit_within_14_days = true;
        }
        if days <= 7 && same_item {
            s.recent_usage_count_7d += 1;
        }
    }
    s
}

fn feedback_bonus(
    items: &[&Clothing],
    adj: &HashMap<String, i32>,
    prefs: &HashMap<String, i32>,
) -> i32 {
    let mut b: i32 = items.iter().filter_map(|i| adj.get(&i.name)).sum();
    for t in outfit_scorer::detect_outfit_tags_pub(items) {
        b += prefs.get(t.as_str()).copied().unwrap_or(0);
    }
    b
}

#[derive(serde::Deserialize)]
struct Run {
    temp: f64,
    candidates: Vec<Vec<String>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let runs: Vec<Run> = serde_json::from_str(&std::fs::read_to_string(&args[1])?)?;
    let user = args.get(2).map(String::as_str).unwrap_or("default");
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let clothes =
        clothing_repo::list_clothing_filtered(&pool, Some("female"), Some(StyleGenre::Glam))
            .await?;
    let mut seasons = HashMap::new();
    let mut worlds = HashMap::new();
    for c in &clothes {
        seasons.insert(
            c.id.clone(),
            clothing_repo::get_seasons(&pool, &c.id)
                .await
                .unwrap_or_default(),
        );
        worlds.insert(
            c.id.clone(),
            clothing_repo::get_texture_worlds(&pool, &c.id)
                .await
                .unwrap_or_default(),
        );
    }
    let adj: HashMap<String, i32> =
        style_engine::db::feedback_repo::get_item_adjustments(&pool, user)
            .await?
            .into_iter()
            .map(|s| (s.item_name, s.score_adjustment))
            .collect();
    let prefs: HashMap<String, i32> =
        style_engine::db::feedback_repo::get_preference_scores(&pool, user)
            .await?
            .into_iter()
            .map(|s| (s.reason_tag, s.score))
            .collect();
    let history = RecommendationHistoryRepo::find_recent_by_user(&pool, user, 50).await?;
    let all_ids: Vec<String> = clothes.iter().map(|c| c.id.clone()).collect();
    let dormant_ids =
        RecommendationHistoryRepo::find_dormant_item_ids(&pool, user, &all_ids).await?;
    let by_name = |n: &str| clothes.iter().find(|c| c.name == n);

    let mut tally: HashMap<&str, [usize; 6]> = HashMap::new();
    for (ri, run) in runs.iter().enumerate() {
        let looks: Vec<Vec<&Clothing>> = run
            .candidates
            .iter()
            .map(|names| names.iter().filter_map(|n| by_name(n)).collect())
            .collect();
        let ctx_of = |items: &[&Clothing]| OutfitContext {
            slots: items
                .iter()
                .map(|c| OutfitSlot {
                    slot: kind(&c.category),
                    clothing: (*c).clone(),
                    seasons: seasons.get(&c.id).cloned().unwrap_or_default(),
                    texture_worlds: worlds.get(&c.id).cloned().unwrap_or_default(),
                })
                .collect(),
            situation: Some("일상".into()),
        };
        let make = |i: usize, items: &[&Clothing], new: bool| {
            let id = |k: SlotKind| {
                items
                    .iter()
                    .find(|c| kind(&c.category) == k)
                    .map(|c| c.id.clone())
            };
            let mut c = OutfitCandidate::new(
                id(SlotKind::Top),
                id(SlotKind::Bottom),
                id(SlotKind::Outer),
                id(SlotKind::Shoes),
                id(SlotKind::Bag),
                i,
            );
            let ctx = ctx_of(items);
            let eval = if new {
                mood_rules::evaluate_for_mood(&ctx, Some(SEASON), Some(StyleGenre::Glam))
            } else {
                engine::evaluate(&ctx, Some(SEASON))
            };
            c.feedback_bonus = feedback_bonus(items, &adj, &prefs);
            c.style_score = eval.score + c.feedback_bonus;
            c
        };
        let mut paths = Vec::new();
        for new in [false, true] {
            let mut cands: Vec<OutfitCandidate> = looks
                .iter()
                .enumerate()
                .map(|(i, l)| make(i, l, new))
                .collect();
            for c in &mut cands {
                if new {
                    // 운영 경로와 같은 함수로 요약한다(옷·보조 아이템 분리).
                    let s = RecommendationHistoryRepo::summarize_candidate_recency(
                        &pool,
                        user,
                        c.top_id.as_deref(),
                        c.bottom_id.as_deref(),
                        c.outer_id.as_deref(),
                        c.shoes_id.as_deref(),
                        c.bag_id.as_deref(),
                    )
                    .await?;
                    c.recency_penalty = calculate_recency_penalty(&s);
                    c.diversity_bonus = calculate_diversity_bonus(&s);
                } else {
                    let s = legacy_summary(c, &history);
                    // 예전 식: 보조 아이템 항목이 없었다.
                    let mut p = calculate_recency_penalty(&s);
                    if s.accessory_used_within_3_days {
                        p -= 3;
                    }
                    c.recency_penalty = p;
                    c.diversity_bonus = calculate_diversity_bonus(&s);
                }
            }
            if new {
                svc::finalize_scores(&mut cands);
            } else {
                for c in &mut cands {
                    c.final_score = c.style_score - c.recency_penalty + c.diversity_bonus;
                }
            }
            cands.sort_by(|a, b| {
                b.final_score
                    .cmp(&a.final_score)
                    .then_with(|| b.style_score.cmp(&a.style_score))
            });
            let unfit = |c: &OutfitCandidate| {
                compute_today_fit(&ctx_of(&looks[c.ai_candidate_index]), run.temp)
                    == TodayFitLevel::Fail
            };
            let servable = if new {
                unfit_gate::keep_wearable_strict(cands, unfit).0
            } else {
                unfit_gate::keep_wearable(cands, unfit).0
            };
            let off = |c: &OutfitCandidate| {
                let items = &looks[c.ai_candidate_index];
                if new {
                    mood_gate::reads_as(StyleGenre::Glam, items) == Some(false)
                } else {
                    !items.iter().any(|i| mood_gate::is_glam_piece(i))
                }
            };
            let servable = unfit_gate::keep_wearable(servable, off).0;
            let todays = svc::select_todays_pick(&servable, run.temp, &clothes);
            let variation = todays
                .as_ref()
                .and_then(|t| svc::select_variation(&servable, &t.candidate));
            let dormant = todays.as_ref().and_then(|t| {
                let mut chosen = vec![&t.candidate];
                if new && let Some(v) = &variation {
                    chosen.push(&v.candidate);
                }
                svc::select_dormant(&servable, &dormant_ids, &chosen)
            });
            paths.push((servable, todays, variation, dormant));
        }
        println!("\n== 재생 {} · {}°C", ri + 1, run.temp);
        for (label, (servable, t, v, d)) in ["변경 전", "변경 후"].iter().zip(&paths) {
            let name = |r: &Option<svc::ModeSelectionResult>| {
                r.as_ref().map(|r| {
                    let items = &looks[r.candidate.ai_candidate_index];
                    let center: Vec<&str> = items
                        .iter()
                        .filter(|i| {
                            matches!(i.category.as_str(), "상의" | "하의" | "아우터")
                                && mood_gate::is_glam_piece(i)
                        })
                        .map(|i| i.category.as_str())
                        .collect();
                    format!(
                        "[{}] {} (최종 {}, 중심 {:?})",
                        r.candidate.ai_candidate_index,
                        items
                            .iter()
                            .map(|i| i.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" / "),
                        r.candidate.final_score,
                        center
                    )
                })
            };
            println!("  {label}: 서빙 가능 {}개", servable.len());
            println!("    오늘의 추천: {}", name(t).unwrap_or("없음".into()));
            println!("    다른 조합:   {}", name(v).unwrap_or("없음".into()));
            println!("    안 입은 옷:  {}", name(d).unwrap_or("없음".into()));
            let e = tally.entry(label).or_default();
            let todays_items = t.as_ref().map(|r| &looks[r.candidate.ai_candidate_index]);
            if let Some(items) = todays_items {
                e[0] += 1;
                if items.iter().any(|i| {
                    matches!(i.category.as_str(), "상의" | "하의" | "아우터")
                        && mood_gate::is_glam_piece(i)
                }) {
                    e[1] += 1;
                }
                if !items.iter().any(|i| {
                    matches!(i.category.as_str(), "상의" | "하의" | "아우터")
                        && mood_gate::is_glam_piece(i)
                }) && items.iter().any(|i| mood_gate::is_glam_piece(i))
                {
                    e[2] += 1;
                }
            }
            let cards: Vec<&Option<svc::ModeSelectionResult>> = vec![t, v, d];
            let sets: Vec<(Option<String>, Option<String>)> = cards
                .iter()
                .filter_map(|r| r.as_ref())
                .map(|r| (r.candidate.top_id.clone(), r.candidate.bottom_id.clone()))
                .collect();
            let uniq: HashSet<_> = sets.iter().collect();
            e[3] += sets.len();
            e[4] += sets.len() - uniq.len();
        }
        // 오늘의 추천이 바뀐 이유 — 변경 후 1위의 감점 차이
        if let (Some(a), Some(b)) = (&paths[0].1, &paths[1].1)
            && a.candidate.ai_candidate_index != b.candidate.ai_candidate_index
        {
            let items = &looks[b.candidate.ai_candidate_index];
            let before = engine::evaluate(&ctx_of(items), Some(SEASON));
            let after =
                mood_rules::evaluate_for_mood(&ctx_of(items), Some(SEASON), Some(StyleGenre::Glam));
            let removed: Vec<String> = before
                .problems
                .iter()
                .filter(|p| {
                    !after
                        .problems
                        .iter()
                        .any(|q| q.rule == p.rule && q.deduction == p.deduction)
                })
                .map(|p| format!("{} −{}", p.rule, p.deduction))
                .collect();
            println!("  바뀐 이유(변경 후 1위의 되돌린 감점): {:?}", removed);
        }
    }
    println!("\n== 합계 (오늘의 추천 기준)");
    for label in ["변경 전", "변경 후"] {
        let e = tally.get(label).copied().unwrap_or_default();
        println!(
            "  {label}: 오늘의 추천 {}회 · 옷에 글램 요소 {} · 가방·신발만 글램 {} · 카드 {}장 중 같은 상하의 중복 {}",
            e[0], e[1], e[2], e[3], e[4]
        );
    }
    Ok(())
}
