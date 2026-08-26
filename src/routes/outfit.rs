use axum::{Json, Router, routing::post};
use chrono::Datelike;

use crate::AppState;
use crate::db::clothing_repo;
use crate::errors::AppError;
use crate::models::outfit::{
    OutfitContext, OutfitEvaluateRequest, OutfitEvaluateResponse, OutfitSlot, SlotKind,
};
use crate::services::llm::LlmTask;
use crate::services::{prompts, style_engine};

pub fn router() -> Router<AppState> {
    Router::new().route("/evaluate", post(evaluate_outfit))
}

async fn evaluate_outfit(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<OutfitEvaluateRequest>,
) -> Result<Json<OutfitEvaluateResponse>, AppError> {
    let slot_defs: Vec<(SlotKind, &str)> = [
        (SlotKind::Top, body.top.as_deref()),
        (SlotKind::Bottom, body.bottom.as_deref()),
        (SlotKind::Outer, body.outer.as_deref()),
        (SlotKind::Shoes, body.shoes.as_deref()),
        (SlotKind::Bag, body.bag.as_deref()),
    ]
    .into_iter()
    .filter_map(|(kind, id)| id.map(|id| (kind, id)))
    .collect();

    if slot_defs.len() < 2 {
        return Err(AppError::BadRequest(
            "최소 2개 이상의 아이템을 지정해주세요".to_string(),
        ));
    }

    let mut slots = Vec::new();
    for (kind, id) in &slot_defs {
        let clothing = clothing_repo::get_clothing_by_id(&state.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("아이템 {} 을(를) 찾을 수 없습니다", id)))?;
        let seasons = clothing_repo::get_seasons(&state.db, id).await?;
        let texture_worlds = clothing_repo::get_texture_worlds(&state.db, id).await?;
        slots.push(OutfitSlot {
            slot: *kind,
            clothing,
            seasons,
            texture_worlds,
        });
    }

    let ctx = OutfitContext {
        slots,
        situation: body.situation.clone(),
    };

    let current_season = current_season_from_month();

    let eval = style_engine::evaluate(&ctx, Some(&current_season));

    let items_desc = style_engine::format_items_description(&ctx.slots);
    let problems_desc = style_engine::format_problems_description(&eval.problems);
    let strengths_desc = style_engine::format_strengths_description(&eval.strengths);
    let suggestions_desc = if eval.suggestions.is_empty() {
        "없음".to_string()
    } else {
        eval.suggestions
            .iter()
            .map(|s| format!("- [{}] {}", s.suggestion_type, s.reason))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let situation_text = body
        .situation
        .as_deref()
        .map(|s| format!("\n상황: {}", s))
        .unwrap_or_default();

    let explanation = match state.llm.ensure_configured(LlmTask::OutfitExplanation) {
        Ok(()) => prompts::generate_outfit_explanation(
            &state.llm,
            &format!("{}{}", items_desc, situation_text),
            eval.verdict.label(),
            &strengths_desc,
            &problems_desc,
            &suggestions_desc,
        )
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to generate outfit explanation: {}", e);
            "설명을 생성할 수 없습니다".to_string()
        }),
        Err(e) => {
            tracing::warn!("Skipping outfit explanation: {}", e);
            "API 키가 설정되지 않아 설명을 생성할 수 없습니다".to_string()
        }
    };

    let verdict_label = eval.verdict.label().to_string();

    Ok(Json(OutfitEvaluateResponse {
        score: eval.score,
        verdict: eval.verdict,
        verdict_label,
        summary: eval.summary,
        problems: eval.problems,
        strengths: eval.strengths,
        suggestions: eval.suggestions,
        explanation,
    }))
}

fn current_season_from_month() -> String {
    let month = chrono::Local::now().month();
    match month {
        3..=5 => "봄",
        6..=8 => "여름",
        9..=11 => "가을",
        _ => "겨울",
    }
    .to_string()
}
