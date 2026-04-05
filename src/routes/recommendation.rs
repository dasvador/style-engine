use axum::{extract::State, routing::post, Json, Router};
use chrono::Datelike;
use tracing::warn;

use crate::db::clothing_repo;
use crate::db::recommendation_history_repo::RecommendationHistoryRepo;
use crate::errors::AppError;
use crate::models::clothing::Clothing;
use crate::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use crate::models::recommendation::{
    OutfitCandidate, OutfitItem, RecommendationRequest, RecommendationResponse,
};
use crate::services::recommendation_service;
use crate::services::{openai, style_engine, weather as weather_service};
use crate::AppState;

/// 싱글유저 앱 — 고정 user_id
const DEFAULT_USER_ID: &str = "default";

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(get_recommendation))
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

    let ctx = OutfitContext {
        slots,
        situation: occasion.map(|s| s.to_string()),
    };

    let eval = style_engine::evaluate(&ctx, current_season);

    let mut candidate = OutfitCandidate::new(top_id, bottom_id, outer_id, shoes_id, bag_id, ai_index);
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
