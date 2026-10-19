use axum::{Json, Router, extract::State, routing::post};
use chrono::Datelike;
use tracing::warn;

use crate::AppState;
use crate::db::clothing_repo;
use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::models::clothing::Clothing;
use crate::models::outfit::Verdict;
use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use crate::models::recommendation::{
    ModeRecommendation, MultiModeRecommendationResponse, OutfitCandidate, OutfitItem,
    RecommendationRequest, RecommendationResponse, ScoringDetail,
};
use crate::models::style_vocab::{Role, Style, StyleGenre, Tone};
use crate::services::cross_mood;
use crate::services::llm::LlmTask;
use crate::services::recommendation_diversity;
use crate::services::recommendation_service;
use crate::services::unfit_gate;
use crate::services::{prompts, style_engine, weather as weather_service};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(get_recommendation))
        .route("/multi", post(get_multi_recommendation))
}

async fn get_recommendation(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RecommendationRequest>,
) -> Result<Json<RecommendationResponse>, AppError> {
    let user_id = &auth.user_id;
    state
        .llm
        .ensure_configured(LlmTask::OutfitCandidates)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

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

    // 3. Get user's clothes (filtered by gender + mood if specified)
    let clothes = if body.gender.is_some() || body.style_mood.is_some() {
        clothing_repo::list_clothing_filtered(
            &state.db,
            body.gender.as_deref(),
            crate::routes::parse_genre(body.style_mood.as_deref())?,
        )
        .await?
    } else {
        clothing_repo::list_clothing(&state.db).await?
    };
    let grouped = build_grouped_clothes(&clothes);

    // 4. Build recency hint from recent history
    let recent_hint = build_recent_hint(&state.db, &clothes, user_id).await;
    let feedback = load_feedback(&state.db, user_id).await;

    // 5. Call OpenAI for 3 candidates
    let multi_result = prompts::get_outfit_candidates(
        &state.llm,
        &weather,
        &grouped,
        body.occasion.as_deref(),
        body.style_preference.as_deref(),
        &recent_hint,
        None,
        false,
        false,
    )
    .await;

    let current_season = current_season_label();

    // 6. Try multi-candidate scoring, fall back to single-candidate
    let (ai_result, selected_candidate) = match multi_result {
        Ok(multi) if !multi.candidates.is_empty() => {
            // Build OutfitCandidates from AI results — 이 경로는 옷장 전체를 보여 줬다.
            let offered = crate::services::item_resolution::OfferedItems::from_clothes(&clothes);
            let mut candidates = Vec::new();
            for (i, ai_candidate) in multi.candidates.iter().enumerate() {
                if let Some(oc) = build_outfit_candidate(
                    &ai_candidate.outfit,
                    &offered,
                    &clothes,
                    &state.db,
                    body.occasion.as_deref(),
                    current_season.as_deref(),
                    i,
                    &feedback,
                    None,
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
                    &state.db, user_id, candidates,
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
            let flat_descriptions = build_flat_descriptions(&clothes);
            let ai_result = prompts::get_outfit_recommendation(
                &state.llm,
                &weather,
                &flat_descriptions,
                body.occasion.as_deref(),
                body.style_preference.as_deref(),
            )
            .await
            .map_err(AppError::Internal)?;
            (ai_result, None)
        }
    };

    // 7. Build response with image_url — 이 요청에서 제공한 옷의 id 로만 연결한다
    //    (`item_resolution`). 연결하지 못한 항목은 버린다. 이름은 옷장의 것을 쓴다.
    let offered_all = crate::services::item_resolution::OfferedItems::from_clothes(&clothes);
    let outfit: Vec<OutfitItem> = ai_result
        .outfit
        .iter()
        .filter_map(|ai_item| {
            let matched = resolve_logged(ai_item, &offered_all)?;
            Some(OutfitItem {
                id: matched.id.clone(),
                category: matched.category.clone(),
                name: matched.name.clone(),
                reason: ai_item.reason.clone(),
                image_url: matched.image_url.clone(),
                material: matched.image_material(),
                wear: None,
            })
        })
        .collect();

    // 8. Save history — either from scored candidate or from raw outfit
    match selected_candidate {
        Some(sel) => {
            let _ = recommendation_service::save_selected_recommendation(&state.db, user_id, &sel)
                .await;
        }
        None => {
            // Fallback: save from matched outfit items
            let top_id = find_slot_id(&outfit, "상의");
            let bottom_id = find_slot_id(&outfit, "하의");
            let outer_id = find_slot_id(&outfit, "아우터");
            let shoes_id = find_slot_id(&outfit, "신발");
            let bag_id = find_slot_id(&outfit, "가방");

            let _ = RecommendationHistoryRepo::insert(
                &state.db,
                user_id,
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
    auth: AuthUser,
    Json(body): Json<RecommendationRequest>,
) -> Result<Json<MultiModeRecommendationResponse>, AppError> {
    let user_id = &auth.user_id;
    state
        .llm
        .ensure_configured(LlmTask::OutfitCandidates)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let region = crate::db::region_repo::get_region(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No region configured. Set a region first.".to_string())
        })?;

    let mut weather = weather_service::fetch_weather(
        &state.http_client,
        &state.kma_api_key,
        region.latitude,
        region.longitude,
    )
    .await
    .map_err(AppError::Internal)?;
    // 개발용: 같은 옷장·피드백으로 기온만 바꿔 추천 경로를 비교한다. 서버를
    // `ALLOW_DEBUG_WEATHER=1` 로 띄웠을 때만 받는다 — 운영에서는 무시된다.
    let debug_t = debug_temperature(
        body.debug_temperature,
        std::env::var("ALLOW_DEBUG_WEATHER").ok().as_deref(),
    );
    if let Some(t) = debug_t {
        tracing::warn!(temperature = t, "debug_temperature 로 기온을 덮어쓴다");
        weather.temperature = t;
        weather.apparent_temperature = t;
    }
    let debug_override = debug_t.is_some();

    let mood = crate::routes::parse_genre(body.style_mood.as_deref())?;
    let mut clothes = if body.gender.is_some() || body.style_mood.is_some() {
        clothing_repo::list_clothing_filtered(&state.db, body.gender.as_deref(), mood).await?
    } else {
        clothing_repo::list_clothing(&state.db).await?
    };

    // 무드 옷장에 이 상황(출근·데이트)에 맞는 옷이 모자란 슬롯은 같은 성별 전체 옷장에서
    // 빌려 온다. 무드가 없으면 이미 전체 옷장이라 빌릴 것이 없다.
    if mood.is_some() {
        let wardrobe =
            clothing_repo::list_clothing_filtered(&state.db, body.gender.as_deref(), None).await?;
        let no_recent = std::collections::HashSet::new();
        let genres = clothing_repo::get_all_style_genres(&state.db).await?;
        let borrowed = crate::services::shortlist::borrow_for_situation(
            &clothes,
            &wardrobe,
            &crate::services::shortlist::ShortlistContext {
                temperature: weather.temperature,
                situation: body.occasion.as_deref(),
                current_season: None,
                recent_item_ids: &no_recent,
            },
            mood,
            &genres,
        );
        if !borrowed.is_empty() {
            tracing::info!(
                borrowed = ?borrowed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
                "무드 옷장에 상황에 맞는 옷이 모자라 전체 옷장에서 빌렸다"
            );
            clothes.extend(borrowed);
        }
    }

    // 팝: 코트를 여미지 않는 기온(12°C 이상)에는 겨울 외투를 무드 옷장에서 뺀다
    // (`mood_gate::hidden_on_mild_pop_day`). 다른 무드의 옷장은 그대로다.
    let dropped = drop_winter_outers_on_mild_pop_days(&mut clothes, mood, weather.temperature);
    if dropped > 0 {
        tracing::info!(dropped, "팝: 온화한 날 겨울 외투 제외");
    }

    let mut recent_hint = build_recent_hint(&state.db, &clothes, user_id).await;
    let current_season = current_season_label();
    // 엣지: 화면에 실제로 보인 카드(`avoid_outfits`)의 룩. 이력에는 대표 카드만 남아 다른
    // 카드로 노출된 하의·배색은 반복으로 잡히지 않았다(2026-10-18 사용자: "늘 블랙 상의 + 차콜
    // 하의"). 후보 지시와 선택에 쓴다. 다른 무드는 그대로다.
    let shown = if mood == Some(StyleGenre::Edgy) {
        shown_looks(&body.avoid_outfits, &clothes, weather.temperature)
    } else {
        Vec::new()
    };
    let feedback = load_feedback(&state.db, user_id).await;

    // 최근 추천 아이템 ID 수집 (shortlist recency penalty용)
    let recent_ids: std::collections::HashSet<String> = {
        let recent = RecommendationHistoryRepo::find_recent_by_user(&state.db, user_id, 7)
            .await
            .unwrap_or_default();
        let mut ids = std::collections::HashSet::new();
        for r in &recent {
            if let Some(id) = &r.top_id {
                ids.insert(id.clone());
            }
            if let Some(id) = &r.bottom_id {
                ids.insert(id.clone());
            }
            if let Some(id) = &r.outer_id {
                ids.insert(id.clone());
            }
            if let Some(id) = &r.shoes_id {
                ids.insert(id.clone());
            }
            if let Some(id) = &r.bag_id {
                ids.insert(id.clone());
            }
        }
        ids
    };

    // Shortlist: 150개 → 슬롯별 top-k
    let sl_ctx = crate::services::shortlist::ShortlistContext {
        temperature: weather.temperature,
        situation: body.occasion.as_deref(),
        current_season: current_season.as_deref(),
        recent_item_ids: &recent_ids,
    };
    // 색 다양성 성별·무드(여성 스트리트·스포티 캐주얼, 남성 아메카지·프레피)는 후보 목록에
    // 색 계열을 남기고 후보 지시의 원칙 5 를 장르의 색 조합으로 바꾼다
    // (`services::mood_palette`). 밝은색을 싫어한다고 적은 사용자에게는 켜지 않는다.
    let dislikes_bright = dislikes_bright_colors(&state.db, user_id).await;
    // 엣지 최근 화면 반복 지시. 밝은색을 싫어한다고 적은 사용자에게는 색을 권하지 않는다.
    if let Some(note) = crate::services::look_signature::repetition_note(&shown, !dislikes_bright) {
        tracing::info!(note = %note.trim_end(), "엣지 최근 화면 반복 지시");
        recent_hint.push_str(&note);
    }
    let palette =
        crate::services::mood_palette::enabled(body.gender.as_deref(), mood, dislikes_bright);
    if crate::services::mood_palette::targets(body.gender.as_deref(), mood) {
        tracing::info!(palette, "무드 색 다양성");
    }
    // 팝도 후보 목록에 색 계열을 남기고 깨진 이름을 뺀다 — 색이 무드의 중심이다.
    let pop = mood == Some(StyleGenre::Pop);
    let shortlist = if palette || pop {
        crate::services::shortlist::build_all_shortlists_with_palette(&clothes, &sl_ctx)
    } else {
        crate::services::shortlist::build_all_shortlists(&clothes, &sl_ctx)
    };
    tracing::info!("{}", shortlist.summary());
    // 글램·엣지는 소재·실루엣·두께·특징까지 넘긴다 — 어떤 옷이 빛을 받는지, 어떤 형태인지가
    // 판단의 중심이라 이름만으로는 모자라다. 다른 무드는 예전과 같은 줄이다.
    let mut grouped = if mood.is_some_and(crate::services::mood_gate::judges) {
        shortlist.to_grouped_detailed()
    } else {
        shortlist.to_grouped()
    };
    if mood == Some(StyleGenre::Glam) {
        grouped.texture_pair = shortlist.glam_texture_pair();
        if let Some(p) = &grouped.texture_pair {
            tracing::info!(outers = ?p.outers, tops = ?p.tops, "글램 소재 대비 재료");
        }
    }
    if palette {
        grouped.colour_items = shortlist.colour_items();
        tracing::info!(colour_items = ?grouped.colour_items, "무드 색 다양성 후보");
    } else if pop {
        grouped.colour_items = shortlist.pop_items();
        tracing::info!(pop_items = ?grouped.colour_items, "팝 후보 옷");
    }

    // 엣지의 색 다양성(후보 지시의 색 문장, 카드의 컬러 우선 선택). 밝은색을 싫어한다고 적은
    // 사용자에게는 켜지 않는다 — 상담(`chat::today_outfit`)과 같은 기준이다. 저장값은 읽기만 한다.
    let edgy_colour = mood == Some(StyleGenre::Edgy) && !dislikes_bright;
    if mood == Some(StyleGenre::Edgy) {
        tracing::info!(edgy_colour, "엣지 색 다양성");
    }

    // LLM: 5후보 생성 (shortlist 기반 카테고리별 그룹 입력). 고른 무드를 함께 넘긴다 —
    // 예전에는 무드가 옷장을 거르는 데만 쓰여, 모델은 무드를 모른 채 "편한 스타일" 로
    // 코디를 짰다.
    let mut ai_candidates = prompts::get_outfit_candidates(
        &state.llm,
        &weather,
        &grouped,
        body.occasion.as_deref(),
        body.style_preference.as_deref(),
        &recent_hint,
        mood,
        edgy_colour,
        palette,
    )
    .await
    .map_err(AppError::Internal)?;

    if ai_candidates.candidates.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!("No candidates from AI")));
    }

    // 후보가 어떤 슬롯을 채웠는지 남긴다. 아우터가 다섯 후보에서 통째로 빠져도
    // 응답만 보고는 알 수 없었던 적이 있어, 초안 단계를 그대로 기록해 둔다.
    for (i, c) in ai_candidates.candidates.iter().enumerate() {
        tracing::debug!(
            "후보 {i}: {}",
            c.outfit
                .iter()
                .map(|o| format!("{}={}", o.category, o.name))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }

    // style_engine 점수 + 이력 penalty/bonus 계산. 모델 응답의 id 는 모델에게 보여 준 후보
    // 목록(슬롯별 shortlist) 안에서만 연결한다 — 무드 옷장 전체로 넓히지 않는다.
    // 이름 충돌은 무드 옷장 전체로 알아본다(고르는 범위는 그대로 shortlist).
    let offered = shortlist.offered_items().with_known(&clothes);
    let mut scored_candidates = Vec::new();
    for (i, ai_c) in ai_candidates.candidates.iter().enumerate() {
        if let Some(oc) = build_outfit_candidate(
            &ai_c.outfit,
            &offered,
            &clothes,
            &state.db,
            body.occasion.as_deref(),
            current_season.as_deref(),
            i,
            &feedback,
            mood,
        )
        .await
        {
            scored_candidates.push(oc);
        }
    }

    // 제한된 후보 보완 — 날씨에 맞는 후보가 하나도 없거나, 글램인데 옷에 글램 요소가 있는
    // 후보가 하나도 없을 때만. 추가 LLM 호출 없이 옷장 안에서 한 슬롯만 바꾼다.
    let supplemented = supplement_candidates(
        &mut ai_candidates.candidates,
        &mut scored_candidates,
        &shortlist,
        &clothes,
        &state.db,
        body.occasion.as_deref(),
        current_season.as_deref(),
        &feedback,
        mood,
        weather.temperature,
    )
    .await;
    if !supplemented.is_empty() {
        tracing::info!(added = ?supplemented, "후보를 옷장 안에서 보완했다");
    }

    let mut reranked = recommendation_service::rerank_candidates_with_history(
        &state.db,
        user_id,
        scored_candidates,
    )
    .await
    .unwrap_or_default();

    // 엣지: 최근 보인 카드와 같아 보이는 후보(같은 배색·하의 형태, 같은 하의)에 노출 반복 감점.
    // 상한이 있어 품질이 크게 낮은 후보가 오르지 않고, 모든 후보가 같은 처지면 순위는 그대로다.
    if !shown.is_empty() {
        for c in &mut reranked {
            let items = candidate_items(c, &clothes);
            let by = |cat: &str| items.iter().copied().find(|i| i.category == cat);
            let (Some(sig), Some(bottom)) = (
                crate::services::look_signature::signature(
                    by("상의"),
                    by("하의"),
                    by("아우터"),
                    Some(weather.temperature),
                ),
                by("하의"),
            ) else {
                continue;
            };
            let p = crate::services::look_signature::shown_look_penalty(&sig, &bottom.id, &shown);
            if p > 0 {
                c.final_score -= p;
                c.recency_penalty += p;
                tracing::info!(
                    idx = c.ai_candidate_index,
                    penalty = p,
                    look = %sig.label(),
                    "엣지 노출 반복 감점"
                );
            }
        }
        reranked.sort_by_key(|c| std::cmp::Reverse(c.final_score));
    }

    // 이미 보여 준 코디와 거의 같은 후보를 뺀다. 무드를 바꿔 요청해도 같은 코디가 다시
    // 나오던 문제 — 화면이 무드마다 따로 요청해서 서버는 앞서 보여 준 것을 몰랐다.
    // 아래 모드 선택 함수는 그대로 두고 그 입력만 거른다.
    let before = reranked.len();
    let reranked = cross_mood::drop_repeats(reranked, &body.avoid_outfits);
    if reranked.len() != before {
        tracing::info!(
            dropped = before - reranked.len(),
            avoid = body.avoid_outfits.len(),
            "이미 보여 준 코디와 겹치는 후보를 뺐다"
        );
    }

    // 휴면 아이템 감지
    let all_ids: Vec<String> = clothes.iter().map(|c| c.id.clone()).collect();
    let dormant_ids =
        RecommendationHistoryRepo::find_dormant_item_ids(&state.db, user_id, &all_ids)
            .await
            .unwrap_or_default();

    // 공유 weather_summary (첫 번째 후보에서)
    let shared_weather = ai_candidates
        .candidates
        .first()
        .map(|c| c.weather_summary.clone())
        .unwrap_or_default();

    // 오늘 입기에 "실패" 인 후보는 세 추천 자리 모두에서 뺀다. 전부 실패여도 되돌려
    // 넣지 않는다 — 위의 보완으로도 없으면 후보 부족으로 알린다. 아래 shadow 로그는
    // 거르기 전 목록으로 남긴다.
    let mut unfit = Vec::with_capacity(reranked.len());
    let mut silhouette_delta = Vec::with_capacity(reranked.len());
    // 오늘 날씨에 경계인 후보. 빼지는 않고 순위에서 민다(`WEATHER_BORDERLINE_PENALTY`).
    let mut borderline: Vec<usize> = Vec::new();
    let mut shadow_scores: Vec<(usize, i32)> = Vec::with_capacity(reranked.len());
    // 무드별 격식 범위·아우터 안의 강한 상의 shadow(`services::mood_shadow`). 운영 점수에
    // 더할 값만 모은다 — 응답 순서에는 쓰지 않는다.
    let mut mood_waivers: Vec<(usize, crate::services::mood_shadow::Waiver)> =
        Vec::with_capacity(reranked.len());
    for c in &reranked {
        let (fail, silhouette) = match crate::services::candidate_pipeline::rebuild_context(
            c,
            &clothes,
            &state.db,
            body.occasion.as_deref(),
        )
        .await
        {
            Some(ctx) => {
                // S2(조용한 아우터 안의 강한 상의) shadow 는 글램·엣지 밖의 무드만 잰다. 두
                // 무드는 `mood_rules` 가 이미 운영 점수에 반영했다.
                if !mood.is_some_and(crate::services::mood_gate::judges) {
                    let eval = style_engine::evaluate(&ctx, current_season.as_deref());
                    mood_waivers.push((
                        c.ai_candidate_index,
                        crate::services::mood_shadow::waiver(&ctx, &eval, mood),
                    ));
                }
                let sub = crate::services::style_engine_v2::compute_subscores(&ctx, None);
                shadow_scores.push((
                    c.ai_candidate_index,
                    crate::services::style_engine_v2::compute_style_score(&sub)
                        + crate::services::serving_ranker::compute_serving_adjustment(&ctx).0,
                ));
                let level =
                    crate::services::serving_ranker::compute_today_fit(&ctx, weather.temperature);
                if level == crate::services::style_engine_v2::TodayFitLevel::Borderline {
                    borderline.push(c.ai_candidate_index);
                }
                (
                    level == crate::services::style_engine_v2::TodayFitLevel::Fail,
                    crate::services::style_engine_v2::silhouette_balance(&ctx),
                )
            }
            None => (false, 0),
        };
        unfit.push((c.ai_candidate_index, fail));
        silhouette_delta.push((c.ai_candidate_index, silhouette));
    }
    let (mut servable, dropped) = unfit_gate::keep_wearable_strict(reranked.clone(), |c| {
        unfit
            .iter()
            .any(|(i, fail)| *i == c.ai_candidate_index && *fail)
    });
    if dropped > 0 {
        tracing::info!(dropped, "오늘 입기 실패 후보를 추천 자리에서 뺐다");
    }
    // 경계 판정을 순위에 반영한다. 예전에는 실패만 뺐고 경계는 아무 영향이 없어, 2°C 의
    // 니트 + 일반 라이더(경계)가 겨울 외투 조합과 같은 자리에서 겨뤘다.
    for c in &mut servable {
        if borderline.contains(&c.ai_candidate_index) {
            c.weather_penalty = WEATHER_BORDERLINE_PENALTY;
            c.final_score -= WEATHER_BORDERLINE_PENALTY;
        }
    }
    servable.sort_by(|a, b| {
        b.final_score
            .cmp(&a.final_score)
            .then_with(|| b.style_score.cmp(&a.style_score))
    });
    if servable.is_empty() {
        tracing::warn!(
            candidates = reranked.len(),
            temperature = weather.temperature,
            "오늘 날씨에 맞는 후보가 없다 — 보완 후에도 없어 후보 부족으로 알린다"
        );
        return Ok(Json(MultiModeRecommendationResponse {
            modes: Vec::new(),
            weather_summary: shared_weather,
            notice: Some(
                "오늘 날씨에 맞는 코디 후보가 부족해요. 다른 무드를 고르거나 옷장에 날씨에 맞는 옷을 더해 보세요."
                    .to_string(),
            ),
            batch_id: None,
        }));
    }

    // 무드를 읽을 수 없는 후보를 뺀다(지금은 글램만 — `services::mood_gate`). 오늘 입기
    // 게이트를 지난 뒤에 걸어, 입을 수 있는 후보 안에서만 고른다.
    let (servable, off_mood) = match mood {
        Some(m) => unfit_gate::keep_wearable(servable, |c| {
            let items: Vec<&Clothing> =
                [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
                    .iter()
                    .filter_map(|id| id.as_ref())
                    .filter_map(|id| clothes.iter().find(|x| &x.id == id))
                    .collect();
            crate::services::mood_gate::reads_as_at(m, &items, Some(weather.temperature))
                == Some(false)
        }),
        None => (servable, 0),
    };
    if off_mood > 0 {
        tracing::info!(dropped = off_mood, mood = ?mood, "무드 요소가 없는 후보를 추천 자리에서 뺐다");
    }
    // 무드 요소가 있는 후보가 하나도 없으면 게이트는 그대로 통과시킨다(빈 추천보다 낫다).
    // 대신 그 사실을 화면에 알린다 — 기본 코디를 글램이라고 내세우지 않는다.
    let mood_shortfall = mood.is_some_and(|m| {
        !servable.iter().any(|c| {
            crate::services::mood_gate::reads_as_at(
                m,
                &candidate_items(c, &clothes),
                Some(weather.temperature),
            ) != Some(false)
        })
    });
    let notice = mood_shortfall.then(|| {
        "옷에 이 무드를 살릴 아이템이 있는 후보가 부족해 기본에 가까운 코디가 섞였어요.".to_string()
    });

    // ─── 순차 모드 선택 (하드 필터링) ───

    // Step 1: 오늘의 추천 — 최고 점수
    let todays =
        recommendation_service::select_todays_pick(&servable, weather.temperature, &clothes);

    // shadow: 실루엣 점수(상하의 부피 관계)를 운영 점수에 더했을 때의 1위. 응답에는 쓰지
    // 않고 로그로만 남긴다 — 사람이 고른 15쌍에서는 일치율이 73.3% → 86.7% 로 올랐지만,
    // 실제 요청에서 1위가 얼마나 바뀌는지 보고 나서 운영에 넣는다.
    let with_silhouette: Vec<_> = servable
        .iter()
        .cloned()
        .map(|mut c| {
            let d = silhouette_delta
                .iter()
                .find(|(i, _)| *i == c.ai_candidate_index)
                .map_or(0, |(_, d)| *d);
            c.style_score += d;
            c.final_score += d;
            c
        })
        .collect();
    let silhouette_pick =
        recommendation_service::select_todays_pick(&with_silhouette, weather.temperature, &clothes);
    let ids = |c: &crate::models::recommendation::OutfitCandidate| {
        [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
            .iter()
            .filter_map(|id| id.as_ref())
            .filter_map(|id| clothes.iter().find(|x| &x.id == id).map(|x| x.name.clone()))
            .collect::<Vec<_>>()
    };
    tracing::info!(
        "silhouette_shadow={}",
        serde_json::json!({
            "production_idx": todays.as_ref().map(|t| t.candidate.ai_candidate_index),
            "silhouette_idx": silhouette_pick.as_ref().map(|t| t.candidate.ai_candidate_index),
            "deltas": silhouette_delta,
            "production_items": todays.as_ref().map(|t| ids(&t.candidate)),
            "silhouette_items": silhouette_pick.as_ref().map(|t| ids(&t.candidate)),
        })
    );

    // Step 2: 다른 조합 — Today와 상의+하의가 다른 후보
    let variation = match &todays {
        Some(t) => recommendation_service::select_variation(&servable, &t.candidate),
        None => None,
    };

    // 엣지: 두 카드가 모두 무채색으로만 보이면 순위가 가까운 컬러 후보를 고른다
    // (`recommendation_service::prefer_visible_color`). 다른 무드는 그대로다.
    let (todays, variation) = if edgy_colour {
        let idx = |r: &Option<recommendation_service::ModeSelectionResult>| {
            r.as_ref().map(|r| r.candidate.ai_candidate_index)
        };
        let before = (idx(&todays), idx(&variation));
        let after = recommendation_service::prefer_visible_color(
            &servable,
            todays,
            variation,
            weather.temperature,
            &clothes,
        );
        if (idx(&after.0), idx(&after.1)) != before {
            tracing::info!(
                ?before,
                after = ?(idx(&after.0), idx(&after.1)),
                "엣지 카드: 순위가 가까운 컬러 후보를 골랐다"
            );
        }
        after
    } else {
        (todays, variation)
    };

    // Step 3: 안 입은 옷 — 휴면 아이템 포함 후보
    let dormant = match &todays {
        Some(t) => {
            let mut chosen = vec![&t.candidate];
            if let Some(v) = &variation {
                chosen.push(&v.candidate);
            }
            recommendation_service::select_dormant(&servable, &dormant_ids, &chosen)
        }
        None => None,
    };

    // ─── shadow: 조용한 아우터 안의 강한 상의(S2, `services::mood_shadow`) ───
    //
    // 글램 밖의 무드에서 이 감점을 되돌렸을 때의 오늘의 추천. 응답에는 쓰지 않는다. 모든
    // 무드에 걸리는 규칙이라 실제 반응으로 따로 판단한다.
    let waiver_of = |idx: usize| {
        mood_waivers
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, w)| *w)
            .unwrap_or_default()
    };
    let mut s2_pool: Vec<_> = servable
        .iter()
        .cloned()
        .map(|mut c| {
            let w = waiver_of(c.ai_candidate_index).total();
            c.style_score += w;
            c.final_score += w;
            c
        })
        .collect();
    s2_pool.sort_by(|a, b| {
        b.final_score
            .cmp(&a.final_score)
            .then_with(|| b.style_score.cmp(&a.style_score))
    });
    let s2_todays =
        recommendation_service::select_todays_pick(&s2_pool, weather.temperature, &clothes);
    let idx_of = |r: &Option<recommendation_service::ModeSelectionResult>| {
        r.as_ref().map(|t| t.candidate.ai_candidate_index)
    };
    let servable_idx: Vec<usize> = servable.iter().map(|c| c.ai_candidate_index).collect();
    tracing::info!(
        "mood_shadow={}",
        serde_json::json!({
            "mood": mood.map(|m| m.as_str()),
            "temperature": weather.temperature,
            "season": current_season,
            "candidates": reranked.iter().map(|c| serde_json::json!({
                "idx": c.ai_candidate_index,
                "items": ids(c),
                "style": c.style_score,
                "final": c.final_score,
                "feedback": c.feedback_bonus,
                "recency": c.recency_penalty,
                "s2": waiver_of(c.ai_candidate_index).inner,
                "servable": servable_idx.contains(&c.ai_candidate_index),
                "focus": ai_candidates.candidates.get(c.ai_candidate_index).and_then(|a| a.focus.clone()),
            })).collect::<Vec<_>>(),
            "production": [idx_of(&todays), idx_of(&variation), idx_of(&dormant)],
            "s2_todays": idx_of(&s2_todays),
        })
    );

    // 결과 조립
    let mode_defs = [
        (
            "todays_pick",
            "오늘의 추천",
            "오늘 날씨와 상황에 가장 잘 맞는 코디",
            &todays,
        ),
        (
            "variation",
            "다른 조합",
            "비슷한 퀄리티로 다른 아이템 조합",
            &variation,
        ),
        (
            "dormant_revival",
            "안 입은 옷 활용",
            "최근에 안 입은 옷으로 괜찮은 코디",
            &dormant,
        ),
    ];

    let shadow_of = |idx: usize| {
        shadow_scores
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, s)| *s)
    };
    let silhouette_of = |idx: usize| {
        silhouette_delta
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, d)| *d)
    };

    let mut modes = Vec::new();
    let todays_idx = todays.as_ref().map(|t| t.candidate.ai_candidate_index);

    for (key, label, description, result) in &mode_defs {
        let (winner, scoring) = match result {
            Some(r) => (&r.candidate, &r.scoring),
            // 고를 후보가 없는 자리는 카드를 내지 않는다. 예전에는 재정렬 목록의 첫
            // 후보로 채웠는데, 그 후보는 날씨·무드 게이트를 거치지 않은 것이었다.
            None => continue,
        };

        let ai_idx = winner.ai_candidate_index;
        let ai_result = &ai_candidates.candidates[ai_idx];
        // 응답 아이템은 채점한 구성(후보의 id) 그대로다. 서버가 채운 신발·가방도 들어가고,
        // 옷장에서 찾지 못한 이름은 빠진다 — 이력·이미지가 같은 구성을 쓴다.
        let outfit = outfit_items_for(winner, ai_result, &clothes, weather.temperature);
        let tips = tips_for(ai_result, &outfit);
        let verdict = Verdict::from_score(scoring.final_score);
        let revival_items = recommendation_diversity::find_dormant_items_in_candidate(
            winner,
            &dormant_ids,
            &clothes,
        );

        let reason = build_mode_reason(key, scoring, &revival_items, todays_idx, ai_idx);

        modes.push(ModeRecommendation {
            mode: key.to_string(),
            mode_label: label.to_string(),
            mode_description: description.to_string(),
            outfit,
            recommendation: ai_result.recommendation.clone(),
            weather_summary: ai_result.weather_summary.clone(),
            tips,
            score: scoring.final_score,
            verdict: verdict.label().to_string(),
            reason,
            revival_items,
            scoring_detail: scoring.clone(),
            shadow_score: shadow_of(ai_idx),
            silhouette_delta: silhouette_of(ai_idx),
            mood_waiver: Some(waiver_of(ai_idx).total()),
            feedback_bonus: winner.feedback_bonus,
        });
    }

    // 이력 저장: Mode 1 (오늘의 추천) winner만
    if let Some(ref t) = todays {
        let _ =
            recommendation_service::save_selected_recommendation(&state.db, user_id, &t.candidate)
                .await;
    }

    // ─── SHADOW EXPERIMENT (S2) — log-only, baseline 영향 0 ───
    // baseline 응답/저장 완료 후 호출. 실패는 모두 swallow.
    crate::services::recommendation_experiment::run_shadow(
        &state.db,
        &reranked,
        &clothes,
        body.occasion.as_deref(),
        current_season.as_deref(),
        weather.temperature,
        todays.as_ref().map(|t| t.candidate.ai_candidate_index),
        variation.as_ref().map(|v| v.candidate.ai_candidate_index),
        dormant.as_ref().map(|d| d.candidate.ai_candidate_index),
        &dormant_ids,
    )
    .await;

    // 응답에 실린 코디를 서버에 남긴다. 두 코디 비교는 화면이 보낸 코디가 아니라 이 기록에서
    // 꺼낸다(`services::outfit_comparison`). 곁가지라 실패해도 추천은 그대로 돌려준다.
    let batch_id = record_batch(
        &state,
        user_id,
        &body,
        mood,
        &weather,
        debug_override,
        &modes,
        &clothes,
    )
    .await;

    Ok(Json(MultiModeRecommendationResponse {
        modes,
        weather_summary: shared_weather,
        notice,
        batch_id,
    }))
}

/// 홈 추천 응답 한 번을 `recommendation_batch` 에 남긴다. 실패하면 `None` — 비교만 못 열 뿐이다.
#[allow(clippy::too_many_arguments)]
async fn record_batch(
    state: &AppState,
    user_id: &str,
    body: &RecommendationRequest,
    mood: Option<StyleGenre>,
    weather: &crate::models::weather::CurrentWeather,
    debug_override: bool,
    modes: &[ModeRecommendation],
    clothes: &[Clothing],
) -> Option<String> {
    use crate::services::outfit_comparison as cmp;

    let candidates = cmp::snapshot_modes(modes, clothes);
    let llm = state.llm.config().task(LlmTask::OutfitCandidates);
    // 알 수 있는 것만 적고, 출처를 같이 남긴다. 프롬프트·엔진에는 버전 표시가 없어 'unknown'.
    let provenance = serde_json::json!({
        "source": "server:/api/recommendation/multi response",
        "candidate_model": {
            "provider": llm.provider.as_str(),
            "model": llm.model,
            "how": "server LLM routing config at request time (provider response not echoed)",
        },
        "prompt_version": "unknown",
        "engine_version": "unknown",
        "app_version": {
            "value": env!("CARGO_PKG_VERSION"),
            "how": "Cargo package version (not a build or commit id)",
        },
    });
    let weather_json = serde_json::json!({
        "temperature": weather.temperature,
        "apparent_temperature": weather.apparent_temperature,
        "weather_code": weather.weather_code,
        "weather_description": weather.weather_description,
        "debug_temperature_override": debug_override,
    });
    let ctx = cmp::BatchContext {
        gender: body.gender.as_deref(),
        mood_key: mood.map(|m| m.as_str()),
        occasion: body.occasion.as_deref(),
        weather: weather_json,
        provenance,
    };
    match cmp::record_batch(&state.db, user_id, &ctx, &candidates).await {
        Ok(id) => id,
        Err(e) => {
            warn!("recommendation_batch 기록 실패: {e}");
            None
        }
    }
}

/// 개발용 기온 덮어쓰기를 받을지. 서버가 `ALLOW_DEBUG_WEATHER=1` 로 떠 있을 때만이다 —
/// 운영에서 요청 본문 하나로 날씨 판정이 바뀌면 안 된다.
fn debug_temperature(requested: Option<f64>, allow: Option<&str>) -> Option<f64> {
    requested.filter(|_| allow == Some("1"))
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

/// 후보가 실제로 입는 옷(상의·하의·아우터·신발·가방 순).
fn candidate_items<'a>(c: &OutfitCandidate, clothes: &'a [Clothing]) -> Vec<&'a Clothing> {
    [&c.top_id, &c.bottom_id, &c.outer_id, &c.shoes_id, &c.bag_id]
        .iter()
        .filter_map(|id| id.as_ref())
        .filter_map(|id| clothes.iter().find(|x| &x.id == id))
        .collect()
}

use crate::services::serving_ranker::WEATHER_BORDERLINE_PENALTY;

/// 서버가 채우거나 바꾼 아이템의 설명. 모델이 쓴 이유가 없는 자리다.
const SERVER_FILLED_REASON: &str = "코디를 완성하려고 옷장에서 골라 넣은 아이템이에요.";

/// 응답 아이템을 **채점한 구성** 으로 만든다.
///
/// 예전 `build_outfit_items` 는 모델의 원래 응답을 다시 읽었다. 그래서 점수를 낼 때 서버가
/// 채운 신발·가방(`select_best_shoe` · `select_best_bag`)이 응답·이미지에서 빠지고,
/// 이력에는 남는 어긋남이 있었다. 이유 문장은 모델이 같은 옷에 쓴 것을 쓰고, 없으면
/// 서버가 채웠다고 적는다.
fn outfit_items_for(
    c: &OutfitCandidate,
    ai_result: &crate::models::recommendation::AiRecommendation,
    clothes: &[Clothing],
    temperature: f64,
) -> Vec<OutfitItem> {
    candidate_items(c, clothes)
        .into_iter()
        .map(|item| {
            let mut reason = ai_result
                .outfit
                .iter()
                .find(|a| a.id.as_deref() == Some(item.id.as_str()))
                .map(|a| a.reason.clone())
                .unwrap_or_else(|| SERVER_FILLED_REASON.to_string());
            // 판정에 쓴 착용 상태를 설명과 이미지에 같이 넘긴다.
            let wear = crate::services::serving_ranker::outer_wear(item, temperature);
            if wear.is_some() {
                // 모델의 설명이 마침표 없이 끝나기도 한다("…강조하기 때문").
                let trimmed = reason.trim_end().to_string();
                reason = if trimmed.is_empty() || trimmed.ends_with(['.', '!', '?']) {
                    trimmed
                } else {
                    format!("{trimmed}.")
                };
                reason.push_str(" 추운 날이라 여며 입는 것을 기준으로 골랐어요.");
                reason = reason.trim_start().to_string();
            }
            OutfitItem {
                id: item.id.clone(),
                category: item.category.clone(),
                name: item.name.clone(),
                reason,
                image_url: item.image_url.clone(),
                material: item.image_material(),
                wear: wear.map(str::to_string),
            }
        })
        .collect()
}

/// 사용자가 밝은색을 싫어한다고 적었는가(`user_style_profile`). 프로필이 없거나 읽지 못하면
/// `false` 다 — 적지 않은 취향을 지어내지 않는다.
async fn dislikes_bright_colors(db: &sqlx::MySqlPool, user_id: &str) -> bool {
    sqlx::query_as::<_, (bool,)>(
        "SELECT dislikes_bright_colors FROM user_style_profile WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .is_some_and(|r| r.0)
}

/// 모델이 썼지만 최종 구성에 없는 옷의 이름.
fn dropped_names(
    ai_result: &crate::models::recommendation::AiRecommendation,
    outfit: &[OutfitItem],
) -> Vec<String> {
    ai_result
        .outfit
        .iter()
        .map(|a| a.name.trim().to_string())
        .filter(|n| !n.is_empty() && !outfit.iter().any(|o| &o.name == n))
        .collect()
}

/// 팁 중 최종 구성에 없는 옷을 말하는 것은 뺀다.
fn tips_for(
    ai_result: &crate::models::recommendation::AiRecommendation,
    outfit: &[OutfitItem],
) -> Vec<String> {
    let gone = dropped_names(ai_result, outfit);
    ai_result
        .tips
        .iter()
        .filter(|t| !gone.iter().any(|g| t.contains(g.as_str())))
        .cloned()
        .collect()
}

/// 후보를 다시 모델 응답 모양으로 적는다. 보완한 후보를 응답·채점 경로에 그대로 태우기
/// 위해서다. 이유 문장은 원래 후보의 것을 옷 id 로 찾아 쓴다.
fn ai_items_of(
    c: &OutfitCandidate,
    source: &crate::models::recommendation::AiRecommendation,
    clothes: &[Clothing],
) -> Vec<crate::models::recommendation::AiOutfitItem> {
    candidate_items(c, clothes)
        .into_iter()
        .map(|item| crate::models::recommendation::AiOutfitItem {
            category: item.category.clone(),
            id: Some(item.id.clone()),
            name: item.name.clone(),
            reason: source
                .outfit
                .iter()
                .find(|a| a.id.as_deref() == Some(item.id.as_str()))
                .map(|a| a.reason.clone())
                .unwrap_or_else(|| SERVER_FILLED_REASON.to_string()),
        })
        .collect()
}

fn slot_name(kind: SlotKind) -> &'static str {
    match kind {
        SlotKind::Top => "상의",
        SlotKind::Bottom => "하의",
        SlotKind::Outer => "아우터",
        SlotKind::Shoes => "신발",
        SlotKind::Bag => "가방",
    }
}

/// 팝 요청에서 코트를 여미지 않는 기온이면 겨울 외투를 무드 옷장에서 뺀다
/// (`mood_gate::hidden_on_mild_pop_day`, 상담과 같은 기준). 뺀 벌 수를 돌려준다.
fn drop_winter_outers_on_mild_pop_days(
    clothes: &mut Vec<Clothing>,
    mood: Option<StyleGenre>,
    temperature: f64,
) -> usize {
    let before = clothes.len();
    clothes.retain(|c| {
        !crate::services::mood_gate::hidden_on_mild_pop_day(c, mood, Some(temperature))
    });
    before - clothes.len()
}

/// 후보 하나의 한 슬롯을 다른 옷으로 바꾼 모델 응답.
fn swapped_ai(
    base: &OutfitCandidate,
    source: &crate::models::recommendation::AiRecommendation,
    clothes: &[Clothing],
    kind: SlotKind,
    to: &Clothing,
    why: &str,
) -> crate::models::recommendation::AiRecommendation {
    let mut items: Vec<_> = ai_items_of(base, source, clothes)
        .into_iter()
        .filter(|a| category_to_slot(&a.category) != Some(kind))
        .collect();
    let removed: Vec<String> = ai_items_of(base, source, clothes)
        .into_iter()
        .filter(|a| category_to_slot(&a.category) == Some(kind))
        .map(|a| a.name)
        .collect();
    items.push(crate::models::recommendation::AiOutfitItem {
        category: to.category.clone(),
        id: Some(to.id.clone()),
        name: to.name.clone(),
        reason: why.to_string(),
    });
    crate::models::recommendation::AiRecommendation {
        recommendation: format!(
            "{} 자리를 {}(으)로 바꾼 코디예요. {}",
            slot_name(kind),
            to.name,
            why
        ),
        outfit: items,
        weather_summary: source.weather_summary.clone(),
        // 바뀐 옷을 말하는 팁은 남기지 않는다.
        tips: source
            .tips
            .iter()
            .filter(|t| !removed.iter().any(|r| t.contains(r.as_str())))
            .cloned()
            .collect(),
        focus: Some(slot_name(kind).to_string()),
    }
}

/// 보완 한 번에 더할 후보 수의 상한. 후보 수를 늘려 문제를 덮지 않도록 작게 둔다.
const MAX_SUPPLEMENTS: usize = 2;

/// 제한된 후보 보완. 더한 후보의 설명을 돌려준다(로그용).
///
/// 1. 날씨: 입을 수 있는 후보가 하나도 없으면, 점수가 높은 실패 후보부터 아우터를
///    shortlist 의 다른 아우터로 바꿔 본다(보온이 모자란 실패가 대부분이다). 최대 2개.
/// 2. 글램: 입을 수 있는 후보 중 옷에 글램 요소가 있는 것이 없으면, 가장 점수가 높은
///    입을 수 있는 후보의 상의·하의·아우터 한 곳을 shortlist 의 글램 옷으로 바꿔 본다.
///    슬롯마다 가장 점수가 높은 것 하나씩, 최대 2개. 특정 상품을 고정하지 않는다.
///
/// 추가 LLM 호출은 하지 않는다. 바꾼 후보도 같은 채점(`build_outfit_candidate`)을 지난다.
#[allow(clippy::too_many_arguments)]
async fn supplement_candidates(
    ai: &mut Vec<crate::models::recommendation::AiRecommendation>,
    scored: &mut Vec<OutfitCandidate>,
    shortlist: &crate::services::shortlist::ShortlistResult<'_>,
    clothes: &[Clothing],
    db: &sqlx::MySqlPool,
    occasion: Option<&str>,
    current_season: Option<&str>,
    feedback: &crate::services::outfit_scorer::FeedbackContext,
    mood: Option<StyleGenre>,
    temperature: f64,
) -> Vec<String> {
    let mut added = Vec::new();
    let fits = |ctx: &OutfitContext| {
        crate::services::serving_ranker::compute_today_fit(ctx, temperature)
            != crate::services::style_engine_v2::TodayFitLevel::Fail
    };
    let mut status = Vec::new();
    for c in scored.iter() {
        let ctx =
            crate::services::candidate_pipeline::rebuild_context(c, clothes, db, occasion).await;
        status.push(ctx.as_ref().is_some_and(&fits));
    }

    // 바꾼 후보를 만들어 입을 수 있으면 돌려준다.
    #[allow(clippy::too_many_arguments)]
    async fn build_variant(
        ai_item: &crate::models::recommendation::AiRecommendation,
        index: usize,
        clothes: &[Clothing],
        db: &sqlx::MySqlPool,
        occasion: Option<&str>,
        current_season: Option<&str>,
        feedback: &crate::services::outfit_scorer::FeedbackContext,
        mood: Option<StyleGenre>,
        temperature: f64,
    ) -> Option<OutfitCandidate> {
        // 서버가 옷장 안에서 만든 후보다(모델 응답이 아니다) — 옷장 안의 id 로 연결한다.
        let offered = crate::services::item_resolution::OfferedItems::from_clothes(clothes);
        let c = build_outfit_candidate(
            &ai_item.outfit,
            &offered,
            clothes,
            db,
            occasion,
            current_season,
            index,
            feedback,
            mood,
        )
        .await?;
        let ctx =
            crate::services::candidate_pipeline::rebuild_context(&c, clothes, db, occasion).await?;
        (crate::services::serving_ranker::compute_today_fit(&ctx, temperature)
            != crate::services::style_engine_v2::TodayFitLevel::Fail)
            .then_some(c)
    }

    // 1. 날씨
    if !status.iter().any(|f| *f) {
        let mut order: Vec<usize> = (0..scored.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(scored[i].style_score));
        for i in order {
            if added.len() >= MAX_SUPPLEMENTS {
                break;
            }
            let base = scored[i].clone();
            let source = ai[base.ai_candidate_index].clone();
            for outer in &shortlist.outers {
                if base.outer_id.as_deref() == Some(outer.id.as_str()) {
                    continue;
                }
                let variant = swapped_ai(
                    &base,
                    &source,
                    clothes,
                    SlotKind::Outer,
                    outer,
                    "오늘 기온에 맞게 겉옷을 더했어요.",
                );
                if let Some(c) = build_variant(
                    &variant,
                    ai.len(),
                    clothes,
                    db,
                    occasion,
                    current_season,
                    feedback,
                    mood,
                    temperature,
                )
                .await
                {
                    added.push(format!("날씨: {} 아우터", outer.name));
                    ai.push(variant);
                    scored.push(c);
                    status.push(true);
                    break;
                }
            }
        }
    }

    // 2. 무드(글램·엣지) — 옷에 무드 요소가 있는 입을 수 있는 후보가 없을 때
    if let Some(m) = mood.filter(|m| crate::services::mood_gate::judges(*m)) {
        let reads = |c: &OutfitCandidate| {
            crate::services::mood_gate::reads_as_at(
                m,
                &candidate_items(c, clothes),
                Some(temperature),
            ) == Some(true)
        };
        let has_mood = scored.iter().zip(&status).any(|(c, ok)| *ok && reads(c));
        let base = scored
            .iter()
            .zip(&status)
            .filter(|(_, ok)| **ok)
            .map(|(c, _)| c)
            .max_by_key(|c| c.style_score)
            .cloned();
        if let (false, Some(base)) = (has_mood, base) {
            let source = ai[base.ai_candidate_index].clone();
            let mut best: Vec<(
                i32,
                SlotKind,
                crate::models::recommendation::AiRecommendation,
                OutfitCandidate,
            )> = Vec::new();
            for (kind, pool) in [
                (SlotKind::Top, &shortlist.tops),
                (SlotKind::Bottom, &shortlist.bottoms),
                (SlotKind::Outer, &shortlist.outers),
            ] {
                // 아우터가 없는 후보에 아우터를 새로 얹지는 않는다 — 날씨가 정한 일이다.
                if kind == SlotKind::Outer && base.outer_id.is_none() {
                    continue;
                }
                let mut slot_best: Option<(
                    i32,
                    crate::models::recommendation::AiRecommendation,
                    OutfitCandidate,
                )> = None;
                for item in pool
                    .iter()
                    .filter(|c| crate::services::mood_gate::is_mood_piece(m, c) == Some(true))
                {
                    let variant = swapped_ai(
                        &base,
                        &source,
                        clothes,
                        kind,
                        item,
                        "시선이 모이는 중심을 옷에 두려고 바꿨어요.",
                    );
                    if let Some(c) = build_variant(
                        &variant,
                        ai.len() + best.len(),
                        clothes,
                        db,
                        occasion,
                        current_season,
                        feedback,
                        mood,
                        temperature,
                    )
                    .await
                        && slot_best
                            .as_ref()
                            .is_none_or(|(s, _, _)| c.style_score > *s)
                    {
                        slot_best = Some((c.style_score, variant, c));
                    }
                }
                if let Some((score, v, c)) = slot_best {
                    best.push((score, kind, v, c));
                }
            }
            best.sort_by_key(|b| std::cmp::Reverse(b.0));
            for (_, kind, variant, mut c) in best.into_iter().take(MAX_SUPPLEMENTS) {
                c.ai_candidate_index = ai.len();
                added.push(format!(
                    "{m}: {} → {}",
                    slot_name(kind),
                    variant.outfit.last().map(|o| o.name.as_str()).unwrap_or("")
                ));
                ai.push(variant);
                scored.push(c);
            }
        }
    }
    added
}

// ─── Helper: AI 후보 → OutfitCandidate (style_engine 점수 포함) ───

/// 피드백이 이 착장에 더하는 점수.
///
/// `outfit_scorer::total_outfit_score_with_feedback` 이 base 점수에 얹는 것과 같은
/// 두 층이다 — 아이템별 누적 보정과 reason tag 선호도. 여기서는 style_engine 이
/// 낸 점수에 얹어야 하므로 보정분만 따로 계산한다.
fn feedback_bonus(
    items: &[&Clothing],
    feedback: &crate::services::outfit_scorer::FeedbackContext,
) -> i32 {
    let mut bonus = 0;
    for item in items {
        if let Some(&adj) = feedback.item_adj.get(&item.name) {
            bonus += adj;
        }
    }
    for tag in crate::services::outfit_scorer::detect_outfit_tags_pub(items) {
        if let Some(&pref) = feedback.preference.get(tag.as_str()) {
            bonus += pref;
        }
    }
    bonus
}

/// 요청 하나에 쓸 피드백 맥락을 읽어 온다.
async fn load_feedback(
    db: &sqlx::MySqlPool,
    user_id: &str,
) -> crate::services::outfit_scorer::FeedbackContext {
    let item_scores = crate::db::feedback_repo::get_item_adjustments(db, user_id)
        .await
        .unwrap_or_default();
    let pref_scores = crate::db::feedback_repo::get_preference_scores(db, user_id)
        .await
        .unwrap_or_default();
    crate::services::outfit_scorer::FeedbackContext {
        item_adj: item_scores
            .into_iter()
            .map(|s| (s.item_name, s.score_adjustment))
            .collect(),
        preference: pref_scores
            .into_iter()
            .map(|s| (s.reason_tag, s.score))
            .collect(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn build_outfit_candidate(
    ai_outfit: &[crate::models::recommendation::AiOutfitItem],
    offered: &crate::services::item_resolution::OfferedItems<'_>,
    clothes: &[Clothing],
    db: &sqlx::MySqlPool,
    occasion: Option<&str>,
    current_season: Option<&str>,
    ai_index: usize,
    feedback: &crate::services::outfit_scorer::FeedbackContext,
    mood: Option<StyleGenre>,
) -> Option<OutfitCandidate> {
    let mut top_id = None;
    let mut bottom_id = None;
    let mut outer_id = None;
    let mut shoes_id = None;
    let mut bag_id = None;
    let mut slots = Vec::new();

    for (slot_kind, clothing) in resolve_outfit(ai_outfit, offered) {
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
    if shoes_id.is_none()
        && let Some((shoe, shoe_seasons, shoe_tw)) =
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

    // LLM이 가방을 안 넣은 경우 → 결정론적 매칭
    if bag_id.is_none()
        && let Some((bag, bag_seasons, bag_tw)) =
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

    let ctx = OutfitContext {
        slots,
        situation: occasion.map(|s| s.to_string()),
    };

    // 무드가 바꾸는 규칙 해석(`services::mood_rules`). 글램이 아니면 운영 평가 그대로다.
    let eval = crate::services::mood_rules::evaluate_for_mood(&ctx, current_season, mood);

    let mut candidate =
        OutfitCandidate::new(top_id, bottom_id, outer_id, shoes_id, bag_id, ai_index);

    // 사용자가 '마음에 들어요' 로 남긴 신호를 점수에 얹는다.
    //
    // 이 보정은 원래 채팅 경로에만 걸려 있었다(`routes::chat`). 홈의 추천은 같은
    // 피드백을 쌓아 두고도 쓰지 않아서, 좋아요를 눌러도 다음 추천이 달라지지
    // 않았다. 같은 `FeedbackContext` 를 여기서도 적용한다.
    let items: Vec<&Clothing> = ctx.slots.iter().map(|s| &s.clothing).collect();
    candidate.feedback_bonus = feedback_bonus(&items, feedback);
    candidate.style_score = eval.score + candidate.feedback_bonus;

    Some(candidate)
}

// ─── Helper: 최근 추천 이력 → LLM 힌트 문자열 ───

async fn build_recent_hint(db: &sqlx::MySqlPool, clothes: &[Clothing], user_id: &str) -> String {
    let recent = RecommendationHistoryRepo::find_recent_by_user(db, user_id, 10)
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
        for slot_id in [
            &row.top_id,
            &row.bottom_id,
            &row.outer_id,
            &row.shoes_id,
            &row.bag_id,
        ]
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
    let shoes: Vec<&Clothing> = clothes.iter().filter(|c| c.category == "신발").collect();

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

    let top_tone = top.and_then(|t| t.tone).unwrap_or(Tone::Mid);
    let bottom_tone = bottom.and_then(|b| b.tone).unwrap_or(Tone::Mid);
    let outfit_style = outer
        .and_then(|o| o.style)
        .or_else(|| top.and_then(|t| t.style))
        .unwrap_or(Style::Basic);

    // 점수 매기기
    let mut scored: Vec<(&Clothing, i32)> = shoes
        .iter()
        .map(|shoe| {
            let mut score = 0i32;
            let shoe_tone = shoe.tone.unwrap_or(Tone::Mid);
            let shoe_role = shoe.role.map(|v| v.as_str()).unwrap_or("");
            let shoe_style = shoe.style.unwrap_or(Style::Basic);

            // 1. 역할 선호: 구조템 > 연결템 > 베이스 > 포인트
            match shoe_role {
                "구조템" => score += 15,
                "연결템" => score += 12,
                "베이스" => score += 8,
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
            if top_tone == Tone::Bright && bottom_tone == Tone::Bright && shoe_tone == Tone::Dark {
                score += 10;
            }

            // 3. 스타일 매치: 코디 스타일과 동일하면 보너스
            if shoe_style == outfit_style || shoe_style == Style::Basic {
                score += 8;
            }
            // 포멀 코디에 러닝화 감점
            if outfit_style == Style::Formal && shoe.name.contains("러닝") {
                score -= 15;
            }

            // 4. neutral 색온도 보너스 (무난한 선택)
            if shoe.color_temperature.as_deref() == Some("neutral") {
                score += 3;
            }

            (*shoe, score)
        })
        .collect();

    scored.sort_by_key(|a| std::cmp::Reverse(a.1));

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
    let bags: Vec<&Clothing> = clothes.iter().filter(|c| c.category == "가방").collect();

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

    let bottom_tone = bottom.and_then(|b| b.tone).unwrap_or(Tone::Mid);
    let outfit_style = outer
        .and_then(|o| o.style)
        .or_else(|| bottom.and_then(|b| b.style))
        .unwrap_or(Style::Basic);

    // 코디에 이미 포인트가 있는지
    let has_accent = [top_id, bottom_id, outer_id]
        .iter()
        .filter_map(|id| id.as_ref())
        .filter_map(|id| clothes.iter().find(|c| c.id == *id))
        .any(|c| matches!(c.role, Some(Role::Accent) | Some(Role::SoftAccent)));

    let shoes_accent =
        shoes.is_some_and(|s| matches!(s.role, Some(Role::Accent) | Some(Role::SoftAccent)));

    let mut scored: Vec<(&Clothing, i32)> = bags
        .iter()
        .map(|bag| {
            let mut score = 0i32;
            let bag_role = bag.role.map(|v| v.as_str()).unwrap_or("");
            let bag_style = bag.style.unwrap_or(Style::Basic);
            let bag_tone = bag.tone.unwrap_or(Tone::Mid);

            // 1. 역할: 구조템/연결템 선호
            match bag_role {
                "구조템" => score += 10,
                "연결템" => score += 8,
                "베이스" => score += 5,
                "포인트" if has_accent || shoes_accent => score -= 10,
                "포인트" => score -= 3,
                "약한포인트" if has_accent => score -= 5,
                _ => {}
            }

            // 2. 스타일 매치
            if bag_style == outfit_style || bag_style == Style::Basic {
                score += 5;
            }

            // 3. 하의와 톤 조화
            if bag_tone == bottom_tone || bag_tone == Tone::Mid {
                score += 3;
            }

            // 4. neutral 색온도 보너스
            if bag.color_temperature.as_deref() == Some("neutral") {
                score += 2;
            }

            (*bag, score)
        })
        .collect();

    scored.sort_by_key(|a| std::cmp::Reverse(a.1));

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

/// 요청의 `avoid_outfits`(화면에 보인 카드의 아이템 id, 오래된 것부터)를 룩으로 복원한다.
/// 이 옷장에 없는 id(다른 무드의 옷)는 무시하고, 하의가 없는 카드는 건너뛴다.
fn shown_looks(
    avoid: &[Vec<String>],
    clothes: &[Clothing],
    temperature: f64,
) -> Vec<crate::services::look_signature::ShownLook> {
    avoid
        .iter()
        .filter_map(|ids| {
            let items: Vec<&Clothing> = ids
                .iter()
                .filter_map(|id| clothes.iter().find(|c| &c.id == id))
                .collect();
            let by = |cat: &str| items.iter().copied().find(|c| c.category == cat);
            let bottom = by("하의")?;
            let signature = crate::services::look_signature::signature(
                by("상의"),
                Some(bottom),
                by("아우터"),
                Some(temperature),
            )?;
            Some(crate::services::look_signature::ShownLook {
                signature,
                bottom_id: bottom.id.clone(),
                bottom_name: bottom.name.clone(),
                top_name: by("상의").map(|c| c.name.clone()),
            })
        })
        .collect()
}

/// 응답 아이템의 슬롯 id. 응답 아이템은 이미 옷장 id 로 연결돼 있다.
fn find_slot_id(outfit: &[OutfitItem], category: &str) -> Option<String> {
    outfit
        .iter()
        .find(|o| o.category == category)
        .map(|o| o.id.clone())
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

/// 모델 응답(또는 서버가 만든 후보)의 아이템들을 슬롯별 옷으로 바꾼다. id 로만 연결하고
/// (`item_resolution`), 연결하지 못한 항목은 빠진다. 같은 슬롯이 두 번 오면 첫 항목만 쓴다 —
/// 예전에는 뒤의 것이 id 를 덮어쓰면서 채점 슬롯에는 두 벌이 다 들어갔다.
fn resolve_outfit<'a>(
    ai_outfit: &[crate::models::recommendation::AiOutfitItem],
    offered: &crate::services::item_resolution::OfferedItems<'a>,
) -> Vec<(SlotKind, &'a Clothing)> {
    let mut out: Vec<(SlotKind, &'a Clothing)> = Vec::new();
    for ai_item in ai_outfit {
        let Some(clothing) = resolve_logged(ai_item, offered) else {
            continue;
        };
        let Some(slot) = category_to_slot(&clothing.category) else {
            continue;
        };
        if out.iter().any(|(s, _)| *s == slot) {
            tracing::warn!(
                event = "ai_item_resolve",
                outcome = "rejected",
                code = "duplicate_slot",
                category = %clothing.category,
                "같은 슬롯이 두 번 왔다 — 첫 항목만 쓴다"
            );
            continue;
        }
        out.push((slot, clothing));
    }
    out
}

/// 모델(또는 서버가 만든 후보)의 한 항목을 이 요청에서 제공한 옷에 연결한다. 결과를 구조화
/// 로그로 남긴다 — 이름이 옷장과 달라도 id 로 연결되면 `name_differs` 로 보인다.
fn resolve_logged<'a>(
    ai_item: &crate::models::recommendation::AiOutfitItem,
    offered: &crate::services::item_resolution::OfferedItems<'a>,
) -> Option<&'a Clothing> {
    use crate::services::item_resolution::{Resolution, resolve_item};
    match resolve_item(
        &ai_item.category,
        ai_item.id.as_deref(),
        &ai_item.name,
        offered,
    ) {
        Resolution::Resolved {
            clothing,
            name_differs,
            ..
        } => {
            if name_differs {
                tracing::info!(
                    event = "ai_item_resolve",
                    outcome = "resolved",
                    name_differs = true,
                    category = %ai_item.category,
                    model_name = %ai_item.name,
                    name = %clothing.name,
                    "모델이 적은 이름이 옷장 이름과 다르다 — id 로 연결했다"
                );
            }
            Some(clothing)
        }
        Resolution::Omitted => None,
        Resolution::Rejected(r) => {
            tracing::warn!(
                event = "ai_item_resolve",
                outcome = "rejected",
                code = r.code(),
                category = %ai_item.category,
                id = ai_item.id.as_deref().unwrap_or(""),
                model_name = %ai_item.name,
                "모델 응답 항목을 연결하지 못했다 — 이름으로 복구하지 않는다"
            );
            None
        }
    }
}

/// 카테고리별 그룹화된 옷장 데이터를 생성. LLM이 슬롯별 후보만 보게 해서 혼동 방지.
fn build_grouped_clothes(clothes: &[Clothing]) -> prompts::GroupedClothes {
    let fmt = |c: &Clothing| {
        format!(
            "- {} | id:{} | role:{} | tone:{} | style:{}",
            c.name,
            c.id,
            c.role.map(|v| v.as_str()).unwrap_or("-"),
            c.tone.map(|v| v.as_str()).unwrap_or("-"),
            c.style.map(|v| v.as_str()).unwrap_or("-"),
        )
    };
    prompts::GroupedClothes {
        tops: clothes
            .iter()
            .filter(|c| c.category == "상의")
            .map(fmt)
            .collect(),
        bottoms: clothes
            .iter()
            .filter(|c| c.category == "하의")
            .map(fmt)
            .collect(),
        outers: clothes
            .iter()
            .filter(|c| c.category == "아우터")
            .map(fmt)
            .collect(),
        shoes: clothes
            .iter()
            .filter(|c| c.category == "신발")
            .map(fmt)
            .collect(),
        bags: clothes
            .iter()
            .filter(|c| c.category == "가방")
            .map(fmt)
            .collect(),
        colour_items: Vec::new(),
        texture_pair: None,
    }
}

/// Fallback용 flat description (단일 후보 추천에 사용)
fn build_flat_descriptions(clothes: &[Clothing]) -> Vec<String> {
    clothes
        .iter()
        .map(|c| {
            format!(
                "{} | id:{} | 카테고리:{} | 톤:{} | 역할:{} | 스타일:{}",
                c.name,
                c.id,
                c.category,
                c.tone.map(|v| v.as_str()).unwrap_or("-"),
                c.role.map(|v| v.as_str()).unwrap_or("-"),
                c.style.map(|v| v.as_str()).unwrap_or("-"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {

    fn wardrobe_item(id: &str, name: &str, category: &str) -> Clothing {
        let now = chrono::NaiveDateTime::default();
        Clothing {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            color: None,
            thickness: crate::models::style_vocab::Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: None,
            style: None,
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: None,
            gender: None,
            style_mood: None,
            visual_weight: None,
            texture_depth: None,
            visual_weight_v2: None,
            texture_depth_v2: None,
            grounding_score: None,
            shadow_tone: None,
            silhouette_volume: None,
            material_primary: None,
            sub_category: None,
            floating_score: None,
            strong_style_score: None,
            texture_keywords: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn ai_item(
        category: &str,
        id: &str,
        name: &str,
        reason: &str,
    ) -> crate::models::recommendation::AiOutfitItem {
        crate::models::recommendation::AiOutfitItem {
            category: category.into(),
            id: Some(id.into()),
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// 서버가 채운 가방은 응답에 들어가고, 모델이 쓴 없는 옷은 빠진다. 응답 아이템은
    /// 채점한 구성(후보의 id)과 같고, 사라진 옷을 말하는 팁도 남지 않는다.
    #[test]
    fn response_items_match_the_scored_outfit() {
        let clothes = vec![
            wardrobe_item("t", "화이트 티", "상의"),
            wardrobe_item("b", "블랙 새틴 스커트", "하의"),
            wardrobe_item("s", "블랙 앵클부츠", "신발"),
            wardrobe_item("g", "블랙 새틴 클러치", "가방"),
        ];
        let ai = crate::models::recommendation::AiRecommendation {
            recommendation: "티에 새틴 스커트".into(),
            outfit: vec![
                ai_item("상의", "t", "화이트 티", "기본"),
                ai_item("하의", "b", "블랙 새틴 스커트", "광택"),
                ai_item("신발", "s", "블랙 앵클부츠", "마무리"),
                // 옷장에 없는 이름 — 채점에서 빠졌다.
                ai_item("가방", "gold", "골드 체인 백", "포인트"),
            ],
            weather_summary: String::new(),
            tips: vec!["골드 체인 백으로 포인트".into(), "부츠로 마무리".into()],
            focus: None,
        };
        // 채점 단계에서 서버가 가방을 채운 후보.
        let c = OutfitCandidate::new(
            Some("t".into()),
            Some("b".into()),
            None,
            Some("s".into()),
            Some("g".into()),
            0,
        );
        let items = outfit_items_for(&c, &ai, &clothes, 20.0);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "화이트 티",
                "블랙 새틴 스커트",
                "블랙 앵클부츠",
                "블랙 새틴 클러치"
            ]
        );
        assert_eq!(items[1].reason, "광택");
        assert_eq!(items[3].reason, SERVER_FILLED_REASON);
        assert_eq!(tips_for(&ai, &items), vec!["부츠로 마무리".to_string()]);
    }

    /// 추운 날 코트는 여며 입은 상태로 판정했으므로, 응답의 코트 칸에 그 상태가 실려
    /// 설명과 이미지 입력에 함께 간다. 따뜻한 날이나 코트가 아닌 칸에는 붙지 않는다.
    #[test]
    fn a_cold_day_coat_carries_its_wear_state() {
        let mut coat = wardrobe_item("o", "블랙 아워글라스 롱 울 코트", "아우터");
        coat.sub_category = Some("롱코트".into());
        coat.texture_keywords = Some("hourglass silhouette, insulated lining".into());
        let clothes = vec![
            wardrobe_item("t", "블랙 원숄더 탑", "상의"),
            wardrobe_item("b", "블랙 와이드 슬랙스", "하의"),
            coat,
        ];
        let ai = crate::models::recommendation::AiRecommendation {
            recommendation: "코트".into(),
            outfit: vec![ai_item("아우터", "o", "블랙 아워글라스 롱 울 코트", "선")],
            weather_summary: String::new(),
            tips: vec![],
            focus: None,
        };
        let c = OutfitCandidate::new(
            Some("t".into()),
            Some("b".into()),
            Some("o".into()),
            None,
            None,
            0,
        );
        let cold = outfit_items_for(&c, &ai, &clothes, 2.0);
        let coat = cold.iter().find(|i| i.category == "아우터").unwrap();
        assert!(coat.wear.as_deref().unwrap().contains("buttoned closed"));
        assert_eq!(
            coat.reason,
            "선. 추운 날이라 여며 입는 것을 기준으로 골랐어요."
        );
        assert!(cold.iter().filter(|i| i.wear.is_some()).count() == 1);
        let warm = outfit_items_for(&c, &ai, &clothes, 15.0);
        assert!(warm.iter().all(|i| i.wear.is_none()));
        assert_eq!(
            warm.iter().find(|i| i.category == "아우터").unwrap().reason,
            "선"
        );
    }

    /// 팝은 12°C 이상에서 겨울 외투(두껍고 보온 확인)를 무드 옷장에서 뺀다. 안감 없는 두꺼운
    /// 아우터·얇은 바람막이는 남고, 12°C 아래와 다른 무드에서는 아무것도 빼지 않는다.
    #[test]
    fn mild_pop_days_drop_only_winter_outers() {
        let mut puffer = item("핑크 오렌지 컬러블록 패딩", "아우터");
        puffer.thickness = Thickness::Thick;
        puffer.texture_keywords = Some("colour-block puffer, down fill, hip-length".into());
        let mut plain_thick = item("두꺼운 데님 재킷", "아우터");
        plain_thick.thickness = Thickness::Thick;
        plain_thick.texture_keywords = Some("heavy denim".into());
        let wardrobe = vec![
            puffer,
            plain_thick,
            item("코발트 옐로 컬러블록 바람막이", "아우터"),
            item("코발트 블루 크롭 니트", "상의"),
        ];
        let run = |mood, t| {
            let mut v = wardrobe.clone();
            let n = drop_winter_outers_on_mild_pop_days(&mut v, mood, t);
            (n, v.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
        };
        let (n, left) = run(Some(StyleGenre::Pop), 19.6);
        assert_eq!(n, 1);
        assert!(!left.contains(&"핑크 오렌지 컬러블록 패딩".to_string()));
        assert_eq!(run(Some(StyleGenre::Pop), 12.0).0, 1);
        assert_eq!(run(Some(StyleGenre::Pop), 8.0).0, 0);
        assert_eq!(run(Some(StyleGenre::Edgy), 19.6).0, 0);
        assert_eq!(run(None, 19.6).0, 0);
    }

    /// 실제로 빠졌던 초안(2026-10-18, 하의 '울혼방' → '울혼합' 오타)을 그대로 재현한다.
    /// - 예전 이름 매칭(정확 → 부분 일치)은 하의를 찾지 못해 상의·하의가 다 있어야 하는
    ///   후보가 통째로 빠졌다.
    /// - 새 형식(id 포함)은 이름 오타와 무관하게 같은 옷으로 연결하고, 표시 이름은 옷장의 것이다.
    /// - id 없는 예전 형식은 새 경로에서 이름으로 받아 주지 않는다(조용한 복구 없음).
    #[test]
    fn the_real_typo_draft_resolves_by_id() {
        let wardrobe = vec![
            wardrobe_item(
                "d8929e84-546b-11f1-af43-264d116ef584",
                "블랙 리브드 슬림 니트",
                "상의",
            ),
            wardrobe_item(
                "8e2f7e38-598a-11f1-af43-264d116ef584",
                "블랙 스트레이트 울혼방 슬랙스",
                "하의",
            ),
            wardrobe_item(
                "0ffd0b00-0000-0000-0000-00000000000f",
                "블랙 아워글라스 롱 울 코트",
                "아우터",
            ),
            wardrobe_item(
                "0ffd0700-0000-0000-0000-00000000000c",
                "블랙 레더 앵클부츠",
                "신발",
            ),
            wardrobe_item(
                "0ffd0b00-0000-0000-0000-00000000000c",
                "실버 메탈릭 미니 숄더백",
                "가방",
            ),
        ];
        let offered = crate::services::item_resolution::OfferedItems::from_clothes(&wardrobe);
        let name_only: crate::models::recommendation::AiRecommendation = serde_json::from_str(
            include_str!("../../tests/fixtures/id_linking/typo_draft_name_only.json"),
        )
        .unwrap();
        let with_ids: crate::models::recommendation::AiRecommendation = serde_json::from_str(
            include_str!("../../tests/fixtures/id_linking/typo_draft_with_ids.json"),
        )
        .unwrap();

        // 예전 규칙(지운 `find_matching_clothing` 과 같은 동작)으로는 하의가 없다.
        let old = |name: &str| {
            wardrobe.iter().find(|c| c.name == name).or_else(|| {
                wardrobe
                    .iter()
                    .find(|c| name.contains(c.name.as_str()) || c.name.contains(name))
            })
        };
        assert!(old("블랙 스트레이트 울혼합 슬랙스").is_none(), "재현 조건");

        let resolved = resolve_outfit(&with_ids.outfit, &offered);
        assert_eq!(resolved.len(), 5);
        let bottom = resolved
            .iter()
            .find(|(s, _)| *s == SlotKind::Bottom)
            .unwrap()
            .1;
        assert_eq!(bottom.id, "8e2f7e38-598a-11f1-af43-264d116ef584");
        assert_eq!(bottom.name, "블랙 스트레이트 울혼방 슬랙스");

        assert!(resolve_outfit(&name_only.outfit, &offered).is_empty());
    }

    /// 홈 → 카드 → 이력: 연결한 id 가 후보·응답 아이템·이력 슬롯 id 까지 그대로 간다. 응답의
    /// 이름은 옷장 이름이고, 모델의 이유 문장은 이름이 틀려도 id 로 같은 옷에 붙는다.
    #[test]
    fn ids_survive_from_draft_to_card_to_history() {
        let clothes = vec![
            wardrobe_item("t1", "블랙 리브드 슬림 니트", "상의"),
            wardrobe_item("t2", "블랙 리브드 슬림 니트", "상의"), // 같은 이름, 다른 옷
            wardrobe_item("b1", "블랙 스트레이트 울혼방 슬랙스", "하의"),
            wardrobe_item("s1", "블랙 레더 앵클부츠", "신발"),
        ];
        let offered = crate::services::item_resolution::OfferedItems::from_clothes(&clothes);
        let ai = crate::models::recommendation::AiRecommendation {
            recommendation: "니트와 슬랙스".into(),
            outfit: vec![
                ai_item("상의", "t2", "블랙 리브드 슬림 니트", "두 번째 니트"),
                ai_item("하의", "b1", "블랙 스트레이트 울혼합 슬랙스", "곧은 하의"),
                ai_item("아우터", "", "", ""), // 정상 생략
                ai_item("신발", "s1", "블랙 레더 앵클부츠", "마무리"),
            ],
            weather_summary: String::new(),
            tips: vec![],
            focus: None,
        };
        let resolved = resolve_outfit(&ai.outfit, &offered);
        let id_of = |k: SlotKind| {
            resolved
                .iter()
                .find(|(s, _)| *s == k)
                .map(|(_, c)| c.id.clone())
        };
        assert_eq!(id_of(SlotKind::Top).as_deref(), Some("t2"));
        assert_eq!(id_of(SlotKind::Outer), None);
        let c = OutfitCandidate::new(
            id_of(SlotKind::Top),
            id_of(SlotKind::Bottom),
            id_of(SlotKind::Outer),
            id_of(SlotKind::Shoes),
            None,
            0,
        );
        let items = outfit_items_for(&c, &ai, &clothes, 20.0);
        let pairs: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.id.as_str(), i.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("t2", "블랙 리브드 슬림 니트"),
                ("b1", "블랙 스트레이트 울혼방 슬랙스"),
                ("s1", "블랙 레더 앵클부츠"),
            ]
        );
        assert_eq!(items[0].reason, "두 번째 니트");
        assert_eq!(items[1].reason, "곧은 하의");
        assert_eq!(find_slot_id(&items, "상의").as_deref(), Some("t2"));
        assert_eq!(find_slot_id(&items, "하의").as_deref(), Some("b1"));
        assert_eq!(find_slot_id(&items, "아우터"), None);
    }

    /// 모델에게 보여 준 목록 밖의 옷(무드 옷장에는 있어도), 다른 슬롯의 id, 틀린 id + 맞는 이름은
    /// 연결하지 않는다. 같은 슬롯이 두 번 오면 첫 항목만 쓴다.
    #[test]
    fn only_offered_ids_in_their_own_slot_are_used() {
        let clothes = [
            wardrobe_item("t1", "화이트 티", "상의"),
            wardrobe_item("t2", "블랙 티", "상의"),
            wardrobe_item("b1", "블랙 데님", "하의"),
            wardrobe_item("o1", "블랙 라이더", "아우터"),
            wardrobe_item("x1", "목록 밖 니트", "상의"), // 무드 옷장에는 있지만 보여 주지 않았다
        ];
        let offered = crate::services::item_resolution::OfferedItems::from_clothes(&clothes[..4]);
        let resolved = resolve_outfit(
            &[
                ai_item("상의", "x1", "목록 밖 니트", ""),
                ai_item("상의", "zz", "화이트 티", ""),
                ai_item("아우터", "b1", "블랙 데님", ""),
                ai_item("상의", "t1", "화이트 티", ""),
                ai_item("상의", "t2", "블랙 티", ""),
                ai_item("하의", "b1", "블랙 데님", ""),
            ],
            &offered,
        );
        let got: Vec<(SlotKind, &str)> =
            resolved.iter().map(|(s, c)| (*s, c.id.as_str())).collect();
        assert_eq!(got, [(SlotKind::Top, "t1"), (SlotKind::Bottom, "b1")]);
    }

    /// 보완한 후보의 설명에는 바뀐 옷이 남지 않는다.
    #[test]
    fn a_swapped_candidate_drops_the_old_item_from_its_text() {
        let clothes = vec![
            wardrobe_item("t", "화이트 티", "상의"),
            wardrobe_item("d", "블랙 데님", "하의"),
            wardrobe_item("k", "블랙 새틴 스커트", "하의"),
        ];
        let ai = crate::models::recommendation::AiRecommendation {
            recommendation: "티에 데님".into(),
            outfit: vec![
                ai_item("상의", "t", "화이트 티", "기본"),
                ai_item("하의", "d", "블랙 데님", "편함"),
            ],
            weather_summary: String::new(),
            tips: vec!["블랙 데님은 밑단을 접어요".into(), "소매를 걷어요".into()],
            focus: None,
        };
        let base = OutfitCandidate::new(Some("t".into()), Some("d".into()), None, None, None, 0);
        let v = swapped_ai(&base, &ai, &clothes, SlotKind::Bottom, &clothes[2], "중심");
        assert!(!v.recommendation.contains("블랙 데님"));
        assert!(v.outfit.iter().all(|o| o.name != "블랙 데님"));
        assert!(v.outfit.iter().any(|o| o.name == "블랙 새틴 스커트"));
        assert_eq!(v.tips, vec!["소매를 걷어요".to_string()]);
        assert_eq!(v.focus.as_deref(), Some("하의"));
    }

    /// 기온 덮어쓰기는 환경변수가 켜졌을 때만 받는다.
    #[test]
    fn debug_temperature_needs_the_env_switch() {
        assert_eq!(debug_temperature(Some(8.0), None), None);
        assert_eq!(debug_temperature(Some(8.0), Some("0")), None);
        assert_eq!(debug_temperature(Some(8.0), Some("1")), Some(8.0));
        assert_eq!(debug_temperature(None, Some("1")), None);
    }

    use super::*;

    /// 홈의 밝은색 비선호는 저장된 프로필을 그대로 읽는다 — 참이면 참, 거짓·프로필 없음은
    /// 거짓. 실제 DB 쿼리를 쓴다. `TEST_DATABASE_URL` 이 없으면 건너뛴다. 넣은 행은 판정
    /// 전에 지운다.
    #[tokio::test]
    async fn bright_colour_preference_is_read_from_the_stored_profile() {
        let Some(url) = std::env::var("TEST_DATABASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
        else {
            eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
            return;
        };
        let db = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("DB 연결");
        let (yes, no, none) = (
            "test-bright-yes-20261017",
            "test-bright-no-20261017",
            "test-bright-none-20261017",
        );
        for (u, v) in [(yes, true), (no, false)] {
            sqlx::query(
                "INSERT INTO user_style_profile (user_id, dislikes_bright_colors) VALUES (?, ?)",
            )
            .bind(u)
            .bind(v)
            .execute(&db)
            .await
            .expect("프로필");
        }
        let got = (
            dislikes_bright_colors(&db, yes).await,
            dislikes_bright_colors(&db, no).await,
            dislikes_bright_colors(&db, none).await,
        );
        sqlx::query("DELETE FROM user_style_profile WHERE user_id IN (?, ?)")
            .bind(yes)
            .bind(no)
            .execute(&db)
            .await
            .expect("정리");
        assert_eq!(got, (true, false, false));
    }
    use crate::models::style_vocab::Thickness;
    use chrono::NaiveDateTime;

    fn ts() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-10-07 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// 이름 매칭만 보는 테스트라 나머지 속성은 전부 비워 둔다.
    fn item(name: &str, category: &str) -> Clothing {
        Clothing {
            id: name.to_string(),
            name: name.to_string(),
            category: category.to_string(),
            gender: None,
            style_mood: None,
            color: None,
            thickness: Thickness::Medium,
            image_url: None,
            tone: None,
            saturation: None,
            style: None,
            weight: None,
            role: None,
            color_temperature: None,
            versatility: None,
            statement_level: None,
            formality_level: None,
            visual_weight: None,
            texture_depth: None,
            visual_weight_v2: None,
            texture_depth_v2: None,
            grounding_score: None,
            shadow_tone: None,
            silhouette_volume: None,
            material_primary: None,
            sub_category: None,
            floating_score: None,
            strong_style_score: None,
            texture_keywords: None,
            created_at: ts(),
            updated_at: ts(),
        }
    }
}
