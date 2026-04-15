use axum::{extract::State, routing::post, Json, Router};
use chrono::Datelike;
use tracing::warn;

use crate::db::clothing_repo;
use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::errors::AppError;
use crate::models::clothing::Clothing;
use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use crate::models::outfit::Verdict;
use crate::models::recommendation::{
    ModeRecommendation, MultiModeRecommendationResponse, OutfitCandidate, OutfitItem,
    RecommendationRequest, RecommendationResponse, ScoringDetail,
};
use crate::services::recommendation_diversity;
use crate::services::recommendation_service;
use crate::services::{openai, style_engine, weather as weather_service};
use crate::AppState;

/// 싱글유저 앱 — 고정 user_id
const DEFAULT_USER_ID: &str = "default";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(get_recommendation))
        .route("/multi", post(get_multi_recommendation))
}

async fn get_recommendation(
    State(state): State<AppState>,
    Json(body): Json<RecommendationRequest>,
) -> Result<Json<RecommendationResponse>, AppError> {
    if state.openai_api_key.is_empty() || state.openai_api_key == "sk-your-key-here" {
        return Err(AppError::BadRequest(
            "OPENAI_API_KEY is not configured".to_string(),
        ));
    }

    // 1. Get region
    let region = crate::db::region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No region configured. Set a region first.".to_string())
        })?;

    // 2. Fetch weather
    let weather = weather_service::fetch_weather(
        &state.http_client,
        &state.kma_api_key,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(AppError::Internal)?;

    // 3. Get user's clothes
    let clothes = clothing_repo::list_clothing(&state.db).await?;
    let clothes_descriptions: Vec<String> = clothes
        .iter()
        .map(|c| {
            format!(
                "{} | 카테고리:{} | 색상:{} | 톤:{} | 색온도:{} | 역할:{} | 스타일:{} | 무게감:{} | 격식:{}",
                c.name,
                c.category,
                c.color.as_deref().unwrap_or("-"),
                c.tone.as_deref().unwrap_or("-"),
                c.color_temperature.as_deref().unwrap_or("-"),
                c.role.as_deref().unwrap_or("-"),
                c.style.as_deref().unwrap_or("-"),
                c.weight.as_deref().unwrap_or("-"),
                c.formality_level.map(|l| l.to_string()).unwrap_or("-".to_string()),
            )
        })
        .collect();

    // 4. Build recency hint from recent history
    let recent_hint = build_recent_hint(&state.db, &clothes).await;

    // 5. Call OpenAI for 3 candidates
    let multi_result = openai::get_outfit_candidates(
        &state.http_client,
        &state.openai_api_key,
        &weather,
        &clothes_descriptions,
        body.occasion.as_deref(),
        body.style_preference.as_deref(),
        &recent_hint,
    )
    .await;

    let current_season = current_season_label();

    // 6. Try multi-candidate scoring, fall back to single-candidate
    let (ai_result, selected_candidate) = match multi_result {
        Ok(multi) if !multi.candidates.is_empty() => {
            // Build OutfitCandidates from AI results
            let mut candidates = Vec::new();
            for (i, ai_candidate) in multi.candidates.iter().enumerate() {
                if let Some(oc) = build_outfit_candidate(
                    &ai_candidate.outfit,
                    &clothes,
                    &state.db,
                    body.occasion.as_deref(),
                    current_season.as_deref(),
                    i,
                )
                .await
                {
                    candidates.push(oc);
                }
            }

            if candidates.is_empty() {
                // All candidates failed — use first AI result as-is
                let first = multi.candidates.into_iter().next().unwrap();
                (first, None)
            } else {
                // Rerank with history penalties
                let reranked = recommendation_service::rerank_candidates_with_history(
                    &state.db,
                    DEFAULT_USER_ID,
                    candidates,
                )
                .await
                .unwrap_or_default();

                let selected = recommendation_service::select_recommendation(&reranked);

                match selected {
                    Some(sel) => {
                        let idx = sel.ai_candidate_index;
                        let ai = multi.candidates.into_iter().nth(idx).unwrap();
                        (ai, Some(sel))
                    }
                    None => {
                        let first = multi.candidates.into_iter().next().unwrap();
                        (first, None)
                    }
                }
            }
        }
        Ok(_) | Err(_) => {
            // Fallback: single candidate via existing function
            if let Err(ref e) = multi_result {
                warn!("Multi-candidate failed, falling back to single: {e}");
            }
            let ai_result = openai::get_outfit_recommendation(
                &state.http_client,
                &state.openai_api_key,
                &weather,
                &clothes_descriptions,
                body.occasion.as_deref(),
                body.style_preference.as_deref(),
            )
            .await
            .map_err(AppError::Internal)?;
            (ai_result, None)
        }
    };

    // 7. Build response with image_url
    let outfit: Vec<OutfitItem> = ai_result
        .outfit
        .into_iter()
        .map(|ai_item| {
            let image_url = find_matching_clothing(&clothes, &ai_item.name)
                .and_then(|c| c.image_url.clone());
            OutfitItem {
                category: ai_item.category,
                name: ai_item.name,
                reason: ai_item.reason,
                image_url,
            }
        })
        .collect();

    // 8. Save history — either from scored candidate or from raw outfit
    match selected_candidate {
        Some(sel) => {
            let _ =
                recommendation_service::save_selected_recommendation(&state.db, DEFAULT_USER_ID, &sel)
                    .await;
        }
        None => {
            // Fallback: save from matched outfit items
            let top_id = find_slot_id(&outfit, "상의", &clothes);
            let bottom_id = find_slot_id(&outfit, "하의", &clothes);
            let outer_id = find_slot_id(&outfit, "아우터", &clothes);
            let shoes_id = find_slot_id(&outfit, "신발", &clothes);
            let bag_id = find_slot_id(&outfit, "가방", &clothes);

            let _ = RecommendationHistoryRepo::insert(
                &state.db,
                DEFAULT_USER_ID,
                top_id.as_deref(),
                bottom_id.as_deref(),
                outer_id.as_deref(),
                shoes_id.as_deref(),
                bag_id.as_deref(),
            )
            .await;
        }
    }

    Ok(Json(RecommendationResponse {
        recommendation: ai_result.recommendation,
        outfit,
        weather_summary: ai_result.weather_summary,
        tips: ai_result.tips,
    }))
}

// ─── 3-모드 추천 (/multi) ───

async fn get_multi_recommendation(
    State(state): State<AppState>,
    Json(body): Json<RecommendationRequest>,
) -> Result<Json<MultiModeRecommendationResponse>, AppError> {
    if state.openai_api_key.is_empty() || state.openai_api_key == "sk-your-key-here" {
        return Err(AppError::BadRequest(
            "OPENAI_API_KEY is not configured".to_string(),
        ));
    }

    let region = crate::db::region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No region configured. Set a region first.".to_string())
        })?;

    let weather = weather_service::fetch_weather(
        &state.http_client,
        &state.kma_api_key,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(AppError::Internal)?;

    let clothes = clothing_repo::list_clothing(&state.db).await?;
    let clothes_descriptions: Vec<String> = clothes
        .iter()
        .map(|c| {
            format!(
                "{} | 카테고리:{} | 색상:{} | 톤:{} | 색온도:{} | 역할:{} | 스타일:{} | 무게감:{} | 격식:{}",
                c.name, c.category,
                c.color.as_deref().unwrap_or("-"),
                c.tone.as_deref().unwrap_or("-"),
                c.color_temperature.as_deref().unwrap_or("-"),
                c.role.as_deref().unwrap_or("-"),
                c.style.as_deref().unwrap_or("-"),
                c.weight.as_deref().unwrap_or("-"),
                c.formality_level.map(|l| l.to_string()).unwrap_or("-".to_string()),
            )
        })
        .collect();

    let recent_hint = build_recent_hint(&state.db, &clothes).await;
    let current_season = current_season_label();

    // LLM: 3후보 생성 (1회 호출)
    let ai_candidates = openai::get_outfit_candidates(
        &state.http_client,
        &state.openai_api_key,
        &weather,
        &clothes_descriptions,
        body.occasion.as_deref(),
        body.style_preference.as_deref(),
        &recent_hint,
    )
    .await
    .map_err(AppError::Internal)?;

    if ai_candidates.candidates.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "No candidates from AI"
        )));
    }

    // style_engine 점수 + 이력 penalty/bonus 계산
    let mut scored_candidates = Vec::new();
    for (i, ai_c) in ai_candidates.candidates.iter().enumerate() {
        if let Some(oc) = build_outfit_candidate(
            &ai_c.outfit,
            &clothes,
            &state.db,
            body.occasion.as_deref(),
            current_season.as_deref(),
            i,
        )
        .await
        {
            scored_candidates.push(oc);
        }
    }

    let reranked = recommendation_service::rerank_candidates_with_history(
        &state.db,
        DEFAULT_USER_ID,
        scored_candidates,
    )
    .await
    .unwrap_or_default();

    // 휴면 아이템 감지
    let all_ids: Vec<String> = clothes.iter().map(|c| c.id.clone()).collect();
    let dormant_ids = RecommendationHistoryRepo::find_dormant_item_ids(
        &state.db,
        DEFAULT_USER_ID,
        &all_ids,
    )
    .await
    .unwrap_or_default();

    // 공유 weather_summary (첫 번째 후보에서)
    let shared_weather = ai_candidates
        .candidates
        .first()
        .map(|c| c.weather_summary.clone())
        .unwrap_or_default();

    // ─── 순차 모드 선택 (하드 필터링) ───

    // Step 1: 오늘의 추천 — 최고 점수
    let todays = recommendation_service::select_todays_pick(
        &reranked,
        weather.temperature,
        &clothes,
    );

    // Step 2: 다른 조합 — Today와 상의+하의가 다른 후보
    let variation = match &todays {
        Some(t) => recommendation_service::select_variation(&reranked, &t.candidate),
        None => None,
    };

    // Step 3: 안 입은 옷 — 휴면 아이템 포함 후보
    let dormant = match &todays {
        Some(t) => {
            recommendation_service::select_dormant(&reranked, &dormant_ids, &t.candidate)
        }
        None => None,
    };

    // 결과 조립
    let mode_defs = [
        ("todays_pick", "오늘의 추천", "오늘 날씨와 상황에 가장 잘 맞는 코디", &todays),
        ("variation", "다른 조합", "비슷한 퀄리티로 다른 아이템 조합", &variation),
        ("dormant_revival", "안 입은 옷 활용", "최근에 안 입은 옷으로 괜찮은 코디", &dormant),
    ];

    let mut modes = Vec::new();
    let todays_idx = todays.as_ref().map(|t| t.candidate.ai_candidate_index);

    for (key, label, description, result) in &mode_defs {
        let (winner, scoring) = match result {
            Some(r) => (&r.candidate, &r.scoring),
            None => {
                // 안 입은 옷 활용은 옵셔널 카드 — 조건 미달 시 노출하지 않음
                if *key == "dormant_revival" {
                    continue;
                }
                // 폴백: 첫 번째 후보
                let c = match reranked.first() {
                    Some(c) => c,
                    None => continue,
                };
                // 인라인 폴백 — 다음 반복에서 사용 안 되므로 임시 할당
                let fallback = recommendation_service::ModeSelectionResult {
                    candidate: c.clone(),
                    scoring: ScoringDetail {
                        style_score: c.style_score,
                        recency_penalty: c.recency_penalty,
                        diversity_bonus: c.diversity_bonus,
                        dormant_bonus: 0,
                        final_score: c.final_score,
                    },
                };
                // 임시 reference 문제 → 직접 빌드
                let ai_idx = c.ai_candidate_index;
                let ai_result = &ai_candidates.candidates[ai_idx];
                let outfit = build_outfit_items(ai_result, &clothes);
                let verdict = Verdict::from_score(c.final_score);
                let revival_items = recommendation_diversity::find_dormant_items_in_candidate(
                    c,
                    &dormant_ids,
                    &clothes,
                );
                let reason = "추천 가능한 코디예요".to_string();
                modes.push(ModeRecommendation {
                    mode: key.to_string(),
                    mode_label: label.to_string(),
                    mode_description: description.to_string(),
                    outfit,
                    recommendation: ai_result.recommendation.clone(),
                    weather_summary: ai_result.weather_summary.clone(),
                    tips: ai_result.tips.clone(),
                    score: fallback.scoring.final_score,
                    verdict: verdict.label().to_string(),
                    reason,
                    revival_items,
                    scoring_detail: fallback.scoring,
                });
                continue;
            }
        };

        let ai_idx = winner.ai_candidate_index;
        let ai_result = &ai_candidates.candidates[ai_idx];
        let outfit = build_outfit_items(ai_result, &clothes);
        let verdict = Verdict::from_score(scoring.final_score);
        let revival_items = recommendation_diversity::find_dormant_items_in_candidate(
            winner,
            &dormant_ids,
            &clothes,
        );

        let reason = build_mode_reason(*key, scoring, &revival_items, todays_idx, ai_idx);

        modes.push(ModeRecommendation {
            mode: key.to_string(),
            mode_label: label.to_string(),
            mode_description: description.to_string(),
            outfit,
            recommendation: ai_result.recommendation.clone(),
            weather_summary: ai_result.weather_summary.clone(),
            tips: ai_result.tips.clone(),
            score: scoring.final_score,
            verdict: verdict.label().to_string(),
            reason,
            revival_items,
            scoring_detail: scoring.clone(),
        });
    }

    // 이력 저장: Mode 1 (오늘의 추천) winner만
    if let Some(ref t) = todays {
        let _ = recommendation_service::save_selected_recommendation(
            &state.db,
            DEFAULT_USER_ID,
            &t.candidate,
        )
        .await;
    }

    Ok(Json(MultiModeRecommendationResponse {
        modes,
        weather_summary: shared_weather,
    }))
}

fn build_mode_reason(
    mode_key: &str,
    scoring: &ScoringDetail,
    revival_items: &[String],
    todays_pick_idx: Option<usize>,
    current_idx: usize,
) -> String {
    match mode_key {
        "todays_pick" => {
            if scoring.recency_penalty == 0 {
                "오늘 날씨와 상황에 가장 잘 어울리는 조합이에요".to_string()
            } else {
                "오늘 조건에 맞으면서 최근 코디와 다른 느낌이에요".to_string()
            }
        }
        "variation" => {
            if todays_pick_idx == Some(current_idx) {
                "현재 옷장에서 가장 좋은 조합이에요. 새 아이템 추가를 추천해요".to_string()
            } else {
                "최근 자주 입은 아이템을 피하고 새로운 조합이에요".to_string()
            }
        }
        "dormant_revival" => {
            if revival_items.is_empty() {
                "모든 아이템이 골고루 사용됐어요. 가장 덜 사용된 조합이에요".to_string()
            } else {
                format!(
                    "최근 14일간 추천되지 않은 {}을(를) 활용했어요",
                    revival_items.join(", ")
                )
            }
        }
        _ => "추천 코디예요".to_string(),
    }
}

fn build_outfit_items(
    ai_result: &crate::models::recommendation::AiRecommendation,
    clothes: &[Clothing],
) -> Vec<OutfitItem> {
    ai_result
        .outfit
        .iter()
        .map(|ai_item| {
            let image_url = find_matching_clothing(clothes, &ai_item.name)
                .and_then(|c| c.image_url.clone());
            OutfitItem {
                category: ai_item.category.clone(),
                name: ai_item.name.clone(),
                reason: ai_item.reason.clone(),
                image_url,
            }
        })
        .collect()
}

// ─── Helper: AI 후보 → OutfitCandidate (style_engine 점수 포함) ───

async fn build_outfit_candidate(
    ai_outfit: &[crate::models::recommendation::AiOutfitItem],
    clothes: &[Clothing],
    db: &sqlx::MySqlPool,
    occasion: Option<&str>,
    current_season: Option<&str>,
    ai_index: usize,
) -> Option<OutfitCandidate> {
    let mut top_id = None;
    let mut bottom_id = None;
    let mut outer_id = None;
    let mut shoes_id = None;
    let mut bag_id = None;
    let mut slots = Vec::new();

    for ai_item in ai_outfit {
        let clothing = match find_matching_clothing(clothes, &ai_item.name) {
            Some(c) => c,
            None => continue,
        };
        let slot_kind = match category_to_slot(&ai_item.category) {
            Some(sk) => sk,
            None => continue,
        };

        // Assign ID to the right slot
        match slot_kind {
            SlotKind::Top => top_id = Some(clothing.id.clone()),
            SlotKind::Bottom => bottom_id = Some(clothing.id.clone()),
            SlotKind::Outer => outer_id = Some(clothing.id.clone()),
            SlotKind::Shoes => shoes_id = Some(clothing.id.clone()),
            SlotKind::Bag => bag_id = Some(clothing.id.clone()),
        }

        let seasons = clothing_repo::get_seasons(db, &clothing.id)
            .await
            .unwrap_or_default();
        let texture_worlds = clothing_repo::get_texture_worlds(db, &clothing.id)
            .await
            .unwrap_or_default();

        slots.push(OutfitSlot {
            slot: slot_kind,
            clothing: clothing.clone(),
            seasons,
            texture_worlds,
        });
    }

    // Need at least top + bottom to score
    if top_id.is_none() || bottom_id.is_none() {
        return None;
    }

    // LLM이 신발을 안 넣은 경우 → 결정론적 매칭
    if shoes_id.is_none() {
        if let Some((shoe, shoe_seasons, shoe_tw)) =
            select_best_shoe(clothes, db, &top_id, &bottom_id, &outer_id).await
        {
            shoes_id = Some(shoe.id.clone());
            slots.push(OutfitSlot {
                slot: SlotKind::Shoes,
                clothing: shoe,
                seasons: shoe_seasons,
                texture_worlds: shoe_tw,
            });
        }
    }

    // LLM이 가방을 안 넣은 경우 → 결정론적 매칭
    if bag_id.is_none() {
        if let Some((bag, bag_seasons, bag_tw)) =
            select_best_bag(clothes, db, &top_id, &bottom_id, &outer_id, &shoes_id).await
        {
            bag_id = Some(bag.id.clone());
            slots.push(OutfitSlot {
                slot: SlotKind::Bag,
                clothing: bag,
                seasons: bag_seasons,
                texture_worlds: bag_tw,
            });
        }
    }

    let ctx = OutfitContext {
        slots,
        situation: occasion.map(|s| s.to_string()),
    };

    let eval = style_engine::evaluate(&ctx, current_season);

    let mut candidate =
        OutfitCandidate::new(top_id, bottom_id, outer_id, shoes_id, bag_id, ai_index);
    candidate.style_score = eval.score;

    Some(candidate)
}

// ─── Helper: 최근 추천 이력 → LLM 힌트 문자열 ───

async fn build_recent_hint(db: &sqlx::MySqlPool, clothes: &[Clothing]) -> String {
    let recent = RecommendationHistoryRepo::find_recent_by_user(db, DEFAULT_USER_ID, 10)
        .await
        .unwrap_or_default();

    if recent.is_empty() {
        return String::new();
    }

    let now = chrono::Local::now().naive_local();
    let mut lines = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for row in &recent {
        let days_ago = (now - row.recommended_at).num_days();
        if days_ago > 3 {
            continue;
        }

        let label = match days_ago {
            0 => "오늘",
            1 => "어제",
            _ => "최근",
        };

        // Collect all slot IDs from this history row
        for slot_id in [&row.top_id, &row.bottom_id, &row.outer_id, &row.shoes_id, &row.bag_id]
            .into_iter()
            .flatten()
        {
            if !seen_ids.insert(slot_id.clone()) {
                continue;
            }
            if let Some(c) = clothes.iter().find(|c| c.id == *slot_id) {
                lines.push(format!("- {} ({})", c.name, label));
            }
        }
    }

    lines.join("\n")
}

// ─── Shoe selection (deterministic fallback) ───

/// LLM이 신발을 안 넣었을 때 결정론적으로 최적 신발 선택
async fn select_best_shoe(
    clothes: &[Clothing],
    db: &sqlx::MySqlPool,
    top_id: &Option<String>,
    bottom_id: &Option<String>,
    outer_id: &Option<String>,
) -> Option<(Clothing, Vec<String>, Vec<String>)> {
    let shoes: Vec<&Clothing> = clothes
        .iter()
        .filter(|c| c.category == "신발")
        .collect();

    if shoes.is_empty() {
        return None;
    }

    let top = top_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));
    let bottom = bottom_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));
    let outer = outer_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));

    let top_tone = top.and_then(|t| t.tone.as_deref()).unwrap_or("중간");
    let bottom_tone = bottom.and_then(|b| b.tone.as_deref()).unwrap_or("중간");
    let outfit_style = outer
        .and_then(|o| o.style.as_deref())
        .or_else(|| top.and_then(|t| t.style.as_deref()))
        .unwrap_or("베이직");

    // 점수 매기기
    let mut scored: Vec<(&Clothing, i32)> = shoes
        .iter()
        .map(|shoe| {
            let mut score = 0i32;
            let shoe_tone = shoe.tone.as_deref().unwrap_or("중간");
            let shoe_role = shoe.role.as_deref().unwrap_or("");
            let shoe_style = shoe.style.as_deref().unwrap_or("베이직");

            // 1. 역할 선호: 구조템 > 연결템 > 밥 > 반찬
            match shoe_role {
                "구조템" => score += 15,
                "연결템" => score += 12,
                "밥" => score += 8,
                _ => {}
            }

            // 2. 톤 대비: 상의+하의와 다른 밝기면 보너스
            let same_as_top = shoe_tone == top_tone;
            let same_as_bottom = shoe_tone == bottom_tone;
            if !same_as_top && !same_as_bottom {
                score += 10; // 둘 다와 다름 → 좋은 대비
            } else if !same_as_top || !same_as_bottom {
                score += 5; // 하나와만 다름
            }
            // 상의+하의 둘 다 밝으면 어두운 신발 강력 선호
            if top_tone == "밝음" && bottom_tone == "밝음" && shoe_tone == "어두움" {
                score += 10;
            }

            // 3. 스타일 매치: 코디 스타일과 동일하면 보너스
            if shoe_style == outfit_style || shoe_style == "베이직" {
                score += 8;
            }
            // 포멀 코디에 러닝화 감점
            if outfit_style == "포멀" && shoe.name.contains("러닝") {
                score -= 15;
            }

            // 4. neutral 색온도 보너스 (무난한 선택)
            if shoe.color_temperature.as_deref() == Some("neutral") {
                score += 3;
            }

            (*shoe, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some((best, _)) = scored.first() {
        let seasons = clothing_repo::get_seasons(db, &best.id)
            .await
            .unwrap_or_default();
        let tw = clothing_repo::get_texture_worlds(db, &best.id)
            .await
            .unwrap_or_default();
        Some(((*best).clone(), seasons, tw))
    } else {
        None
    }
}

/// LLM이 가방을 안 넣었을 때 결정론적 최적 가방 선택
async fn select_best_bag(
    clothes: &[Clothing],
    db: &sqlx::MySqlPool,
    top_id: &Option<String>,
    bottom_id: &Option<String>,
    outer_id: &Option<String>,
    shoes_id: &Option<String>,
) -> Option<(Clothing, Vec<String>, Vec<String>)> {
    let bags: Vec<&Clothing> = clothes
        .iter()
        .filter(|c| c.category == "가방")
        .collect();

    if bags.is_empty() {
        return None;
    }

    let bottom = bottom_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));
    let outer = outer_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));
    let shoes = shoes_id
        .as_ref()
        .and_then(|id| clothes.iter().find(|c| c.id == *id));

    let bottom_tone = bottom.and_then(|b| b.tone.as_deref()).unwrap_or("중간");
    let outfit_style = outer
        .and_then(|o| o.style.as_deref())
        .or_else(|| bottom.and_then(|b| b.style.as_deref()))
        .unwrap_or("베이직");

    // 코디에 이미 반찬이 있는지
    let has_accent = [top_id, bottom_id, outer_id]
        .iter()
        .filter_map(|id| id.as_ref())
        .filter_map(|id| clothes.iter().find(|c| c.id == *id))
        .any(|c| matches!(c.role.as_deref(), Some("반찬") | Some("약한반찬")));

    let shoes_accent = shoes.is_some_and(|s| {
        matches!(s.role.as_deref(), Some("반찬") | Some("약한반찬"))
    });

    let mut scored: Vec<(&Clothing, i32)> = bags
        .iter()
        .map(|bag| {
            let mut score = 0i32;
            let bag_role = bag.role.as_deref().unwrap_or("");
            let bag_style = bag.style.as_deref().unwrap_or("베이직");
            let bag_tone = bag.tone.as_deref().unwrap_or("중간");

            // 1. 역할: 구조템/연결템 선호
            match bag_role {
                "구조템" => score += 10,
                "연결템" => score += 8,
                "밥" => score += 5,
                "반찬" if has_accent || shoes_accent => score -= 10,
                "반찬" => score -= 3,
                "약한반찬" if has_accent => score -= 5,
                _ => {}
            }

            // 2. 스타일 매치
            if bag_style == outfit_style || bag_style == "베이직" {
                score += 5;
            }

            // 3. 하의와 톤 조화
            if bag_tone == bottom_tone || bag_tone == "중간" {
                score += 3;
            }

            // 4. neutral 색온도 보너스
            if bag.color_temperature.as_deref() == Some("neutral") {
                score += 2;
            }

            (*bag, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some((best, _)) = scored.first() {
        let seasons = clothing_repo::get_seasons(db, &best.id)
            .await
            .unwrap_or_default();
        let tw = clothing_repo::get_texture_worlds(db, &best.id)
            .await
            .unwrap_or_default();
        Some(((*best).clone(), seasons, tw))
    } else {
        None
    }
}

// ─── Utilities ───

fn find_slot_id(outfit: &[OutfitItem], category: &str, clothes: &[Clothing]) -> Option<String> {
    outfit
        .iter()
        .find(|o| o.category == category)
        .and_then(|o| find_matching_clothing(clothes, &o.name))
        .map(|c| c.id.clone())
}

fn category_to_slot(cat: &str) -> Option<SlotKind> {
    match cat {
        "상의" => Some(SlotKind::Top),
        "하의" => Some(SlotKind::Bottom),
        "아우터" => Some(SlotKind::Outer),
        "신발" => Some(SlotKind::Shoes),
        "가방" => Some(SlotKind::Bag),
        _ => None,
    }
}

fn current_season_label() -> Option<String> {
    let month = chrono::Local::now().month();
    Some(
        match month {
            3..=5 => "봄",
            6..=8 => "여름",
            9..=11 => "가을",
            _ => "겨울",
        }
        .to_string(),
    )
}

/// Find a matching clothing record by name.
/// Tries exact match first, then substring contains.
fn find_matching_clothing<'a>(clothes: &'a [Clothing], name: &str) -> Option<&'a Clothing> {
    if let Some(c) = clothes.iter().find(|c| c.name == name) {
        return Some(c);
    }
    clothes
        .iter()
        .find(|c| name.contains(&c.name) || c.name.contains(name))
}
