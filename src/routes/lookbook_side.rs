//! 룩북 사진의 90도 옆모습.
//!
//!   GET  /api/lookbook/{id}/side  — 지금 상태만 본다(생성하지 않는다)
//!   POST /api/lookbook/{id}/side  — 없으면 한 장 만든다. 있거나 만드는 중이면 그 상태를 돌려준다
//!
//! 원본은 룩에 붙은 착용 사진(`lookbook_look.image_path`) 그대로다. 그 사진을 images/edits 에 넣어
//! "카메라만 옆으로 옮긴" 사진을 low 1024x1536 으로 한 장 만든다. 원본 파일은 읽기만 한다.
//!
//! 결과는 계정·원본 사진별로 `lookbook_side_image` 에 남는다. 생성은 그 행을 먼저 'pending' 으로 잡은
//! 요청만 한다 — 새로고침·서버 재시작·동시 클릭이 같은 사진을 다시 사지 않는다. 생성 도중 서버가
//! 꺼져 남은 pending 은 `STALE_PENDING_MINUTES` 뒤에 다음 요청이 다시 잡는다.
//!
//! 대상: 내 룩이고, 그 사진이 내 생성 캐시의 착용 사진(`presentation = 'model'`, 검증 통과)이며,
//! 파일이 실제로 있는 PNG 일 때만. 사진 없는 룩·의류 배치(flat lay)·남의 사진은 만들지 않는다.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::services::llm::{ImageEditRequest, LlmTask};

/// 한 원본에 대해 시도할 최대 횟수(일시 오류 재시도 포함). 넘으면 더 만들지 않는다.
const MAX_ATTEMPTS: i32 = 3;

/// 이보다 오래된 pending 은 생성 도중 끊긴 것으로 보고 다시 잡는다. LLM 호출은 60초 타임아웃에
/// 재시도 두 번이라 길어야 3~4분이다.
const STALE_PENDING_MINUTES: i32 = 10;

/// 옆모습 편집 문구. 어떤 코디에도 쓰도록 옷·인물을 적지 않는다 — 그건 입력 사진이 정한다.
/// (2026-11-18 단건 시험의 문구에서 특정 옷·머리·장소 묘사를 걷어낸 것)
pub(crate) const SIDE_PROMPT: &str = "Re-photograph this exact scene from a new camera position: \
the photographer walks 90 degrees around the model and shoots a true strict side profile view, \
full body from head to toe. Face, shoulders, torso, hips, knees and both feet are all in exact \
90-degree profile together, facing toward one edge of the frame. The model does not look at the \
camera; she looks straight ahead. Only one eye is visible; the nose, lips and chin form a clean \
profile silhouette. The far shoulder and far hip are hidden behind the near shoulder and near hip, \
with minimal view of the front of the body. The far foot is mostly hidden behind the near one. If \
she carries a bag, shoot from the side where it hangs so it stays visible. Keep the same person: \
same face, hair, makeup, expression and body shape, and the same pose as far as a side view allows. \
Keep every garment, shoe and accessory exactly as worn, with the same colours, fabrics, textures, \
fit, lengths, how each piece is fastened or worn open, and any logo unchanged. Do not invent logos, \
prints, pockets or decorations. Same location, same light and colours; the background perspective \
changes naturally to match the moved camera. Photorealistic, full body fully in frame, no text.";

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[derive(Debug, Serialize, PartialEq)]
pub struct SideResponse {
    /// pending=만드는 중 / ready=있음 / failed=만들지 못함 / blocked=안전 검사 차단 / budget=예산 소진
    pub status: &'static str,
    pub image_path: Option<String>,
    /// 다시 누르면 다시 만들어 볼 수 있는가.
    pub retryable: bool,
}

impl SideResponse {
    fn of(status: &'static str, image_path: Option<String>, retryable: bool) -> Self {
        Self {
            status,
            image_path,
            retryable,
        }
    }
}

/// 저장된 행 하나를 화면 상태로. 행이 없으면 아직 만든 적이 없는 것이다.
fn state_of(row: Option<&SideRow>) -> SideResponse {
    match row {
        None => SideResponse::of("none", None, true),
        Some(r) => match r.status.as_str() {
            "ready" => SideResponse::of("ready", r.image_path.clone(), false),
            "pending" => SideResponse::of("pending", None, false),
            "blocked" => SideResponse::of("blocked", None, false),
            _ => SideResponse::of("failed", None, r.attempts < MAX_ATTEMPTS),
        },
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SideRow {
    status: String,
    image_path: Option<String>,
    attempts: i32,
}

/// `/static/images/<uuid>.png` 꼴이고 파일이 실제로 있는 PNG 면 그 바이트. 심볼릭 링크는 따르지 않는다.
async fn read_server_png(path: &str) -> Option<Vec<u8>> {
    let file = path.strip_prefix("/static/images/")?;
    if !crate::routes::images::valid_name(file) {
        return None;
    }
    let disk = std::path::Path::new("static/images").join(file);
    match tokio::fs::symlink_metadata(&disk).await {
        Ok(meta) if meta.is_file() => {}
        _ => return None,
    }
    let bytes = tokio::fs::read(&disk).await.ok()?;
    bytes.starts_with(&PNG_MAGIC).then_some(bytes)
}

/// 목록 화면용 가벼운 확인 — 경로 꼴과 파일 존재만 본다.
pub(crate) fn source_file_exists(path: &str) -> bool {
    path.strip_prefix("/static/images/")
        .filter(|f| crate::routes::images::valid_name(f))
        .and_then(|f| std::fs::symlink_metadata(std::path::Path::new("static/images").join(f)).ok())
        .is_some_and(|m| m.is_file())
}

/// 내 룩의 원본 사진 경로. 룩이 없으면 404, 옆모습을 만들 수 없는 사진이면 422.
async fn eligible_source(
    db: &sqlx::MySqlPool,
    owner: &str,
    look_id: &str,
) -> Result<String, AppError> {
    let look: Option<(Option<String>,)> =
        sqlx::query_as("SELECT image_path FROM lookbook_look WHERE id = ? AND user_id = ?")
            .bind(look_id)
            .bind(owner)
            .fetch_optional(db)
            .await?;
    let Some((source,)) = look else {
        return Err(AppError::NotFound(
            "이 코디를 찾을 수 없습니다.".to_string(),
        ));
    };
    let unavailable = || AppError::Validation("이 사진은 옆모습을 만들 수 없어요.".to_string());
    let source = source.ok_or_else(unavailable)?;
    // 내가 만든 착용 사진만. 의류 배치·검증 실패·남의 사진은 대상이 아니다.
    let model_photo: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM outfit_image WHERE user_id = ? AND image_path = ? \
           AND presentation = 'model' AND verification_status = 'passed')",
    )
    .bind(owner)
    .bind(&source)
    .fetch_one(db)
    .await?;
    if !model_photo || !source_file_exists(&source) {
        return Err(unavailable());
    }
    Ok(source)
}

async fn load_row(
    db: &sqlx::MySqlPool,
    owner: &str,
    source: &str,
) -> Result<Option<SideRow>, AppError> {
    Ok(sqlx::query_as::<_, SideRow>(
        "SELECT CAST(status AS CHAR) AS status, image_path, attempts FROM lookbook_side_image \
         WHERE user_id = ? AND source_path = ?",
    )
    .bind(owner)
    .bind(source)
    .fetch_optional(db)
    .await?)
}

pub async fn side_status(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SideResponse>, AppError> {
    let source = eligible_source(&state.db, &auth.user_id, &id).await?;
    let row = load_row(&state.db, &auth.user_id, &source).await?;
    Ok(Json(state_of(row.as_ref())))
}

/// 생성을 잡는다. 잡았으면 `true` — 이 요청만 모델을 부른다.
///
/// 새 행은 INSERT IGNORE 로, 있던 행은 조건부 UPDATE 한 문장으로 잡는다. 둘 다 한 문장이라 동시에
/// 들어온 요청 중 하나만 성공한다.
async fn claim(
    db: &sqlx::MySqlPool,
    owner: &str,
    source: &str,
    token: &str,
) -> Result<bool, AppError> {
    let inserted = sqlx::query(
        "INSERT IGNORE INTO lookbook_side_image \
         (user_id, source_path, status, attempts, claim_token, claimed_at) \
         VALUES (?, ?, 'pending', 1, ?, NOW())",
    )
    .bind(owner)
    .bind(source)
    .bind(token)
    .execute(db)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(true);
    }
    let retaken = sqlx::query(
        "UPDATE lookbook_side_image \
         SET status = 'pending', image_path = NULL, attempts = attempts + 1, claim_token = ?, claimed_at = NOW() \
         WHERE user_id = ? AND source_path = ? AND attempts < ? \
           AND (status = 'failed' \
                OR (status = 'pending' AND claimed_at < NOW() - INTERVAL ? MINUTE))",
    )
    .bind(token)
    .bind(owner)
    .bind(source)
    .bind(MAX_ATTEMPTS)
    .bind(STALE_PENDING_MINUTES)
    .execute(db)
    .await?;
    Ok(retaken.rows_affected() == 1)
}

/// 잡은 행의 결과를 적는다. 그사이 다른 요청이 오래된 pending 으로 보고 다시 잡았으면 0행이다.
async fn finish(
    db: &sqlx::MySqlPool,
    owner: &str,
    source: &str,
    token: &str,
    status: &str,
    image_path: Option<&str>,
) -> bool {
    sqlx::query(
        "UPDATE lookbook_side_image SET status = ?, image_path = ?, prompt_text = ?, claim_token = NULL \
         WHERE user_id = ? AND source_path = ? AND claim_token = ?",
    )
    .bind(status)
    .bind(image_path)
    .bind(SIDE_PROMPT)
    .bind(owner)
    .bind(source)
    .bind(token)
    .execute(db)
    .await
    .map(|r| r.rows_affected() == 1)
    .unwrap_or(false)
}

pub async fn ensure_side(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SideResponse>, AppError> {
    let owner = auth.user_id.as_str();
    let source = eligible_source(&state.db, owner, &id).await?;

    let row = load_row(&state.db, owner, &source).await?;
    match row.as_ref() {
        // 있는 사진은 그대로 준다. 파일이 사라졌으면 다시 만들 수 있게 실패로 돌린다.
        Some(r) if r.status == "ready" => {
            if r.image_path.as_deref().is_some_and(source_file_exists) {
                return Ok(Json(state_of(Some(r))));
            }
            sqlx::query(
                "UPDATE lookbook_side_image SET status = 'failed', image_path = NULL \
                 WHERE user_id = ? AND source_path = ? AND status = 'ready'",
            )
            .bind(owner)
            .bind(&source)
            .execute(&state.db)
            .await?;
        }
        Some(r) if r.status == "blocked" => return Ok(Json(state_of(Some(r)))),
        Some(r) if r.status == "failed" && r.attempts >= MAX_ATTEMPTS => {
            return Ok(Json(state_of(Some(r))));
        }
        _ => {}
    }

    // 예산이 다 찼으면 잡지 않는다 — 다음에 다시 누를 수 있다.
    use crate::routes::chat as img;
    let daily = img::budget_from_env("IMAGE_DAILY_BUDGET_USD", img::DEFAULT_DAILY_BUDGET_USD);
    let monthly = img::budget_from_env("IMAGE_MONTHLY_BUDGET_USD", img::DEFAULT_MONTHLY_BUDGET_USD);
    if img::BUDGET_ENFORCED
        && !img::budget_allows_image(&img::image_spend(&state.db).await, daily, monthly)
    {
        return Ok(Json(SideResponse::of("budget", None, true)));
    }

    let token = uuid::Uuid::new_v4().to_string();
    if !claim(&state.db, owner, &source, &token).await? {
        // 다른 요청이 만드는 중이거나, 더 만들 수 없는 상태다.
        let row = load_row(&state.db, owner, &source).await?;
        return Ok(Json(state_of(row.as_ref())));
    }

    let Some(base_png) = read_server_png(&source).await else {
        finish(&state.db, owner, &source, &token, "failed", None).await;
        return Err(AppError::Validation(
            "이 사진은 옆모습을 만들 수 없어요.".to_string(),
        ));
    };

    let started = std::time::Instant::now();
    let edited = state
        .llm
        .edit_image_as(
            LlmTask::ImageAngle,
            &ImageEditRequest {
                base_png,
                prompt: SIDE_PROMPT.to_string(),
                size: "1024x1536".to_string(),
                quality: "low".to_string(),
            },
        )
        .await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let (status, saved) = match edited {
        Ok(image) => {
            record_spend(&state, &image.usage).await;
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(image.b64_png.as_str())
                .ok()
                .filter(|b| b.starts_with(&PNG_MAGIC));
            // 받은 PNG 바이트를 그대로 쓴다(출처 메타데이터를 지우는 변환을 하지 않는다).
            let saved = match bytes {
                Some(bytes) => {
                    let file = format!("{}.png", uuid::Uuid::new_v4());
                    match tokio::fs::write(
                        std::path::Path::new("static/images").join(&file),
                        &bytes,
                    )
                    .await
                    {
                        Ok(()) => Some(format!("/static/images/{file}")),
                        Err(e) => {
                            tracing::warn!("옆모습 저장 실패: {e}");
                            None
                        }
                    }
                }
                None => {
                    tracing::warn!("옆모습 응답이 PNG 가 아니다");
                    None
                }
            };
            (if saved.is_some() { "ready" } else { "failed" }, saved)
        }
        Err(e) => {
            let d = e.api_detail().cloned().unwrap_or_default();
            tracing::warn!(
                event = "lookbook_side_failed",
                blocked = e.is_moderation_blocked(),
                code = d.code.as_deref().unwrap_or(""),
                request_id = d.request_id.as_deref().unwrap_or(""),
                "옆모습 생성 실패: {e}"
            );
            (
                if e.is_moderation_blocked() {
                    "blocked"
                } else {
                    "failed"
                },
                None,
            )
        }
    };

    if !finish(&state.db, owner, &source, &token, status, saved.as_deref()).await {
        // 그사이 다른 요청이 다시 잡았다. 이 결과는 버리고 그쪽 결과를 따른다.
        if let Some(p) = &saved {
            let _ = tokio::fs::remove_file(p.trim_start_matches('/')).await;
        }
        let row = load_row(&state.db, owner, &source).await?;
        return Ok(Json(state_of(row.as_ref())));
    }
    tracing::info!(
        event = "lookbook_side",
        status,
        source = %source,
        image = saved.as_deref().unwrap_or(""),
        elapsed_ms,
        model = %state.llm.config().task(LlmTask::ImageAngle).model,
        "룩북 옆모습"
    );
    let row = load_row(&state.db, owner, &source).await?;
    Ok(Json(state_of(row.as_ref())))
}

/// 이미지 예산 원장에 더한다. 편집은 입력 대부분이 원본 그림(이미지 단가)이라 그 차이를 얹는다
/// (`chat::ImageGenMetrics::edit_cost_usd` 와 같은 셈).
async fn record_spend(state: &AppState, usage: &crate::services::llm::Usage) {
    use crate::services::llm::usage::estimate_cost_usd;
    let model = &state.llm.config().task(LlmTask::ImageAngle).model;
    let Some(base) = estimate_cost_usd(model, usage) else {
        tracing::warn!("옆모습 비용을 추정할 수 없어 예산 원장에 반영하지 못했다");
        return;
    };
    let cost = base
        + usage.input_tokens as f64 / 1_000_000.0
            * crate::routes::chat::EDIT_IMAGE_INPUT_SURCHARGE_PER_MTOK;
    let _ = sqlx::query(
        "INSERT INTO image_daily_spend (spend_date, cost_usd, generate_calls, verify_calls) \
         VALUES (CURRENT_DATE, ?, 1, 0) AS new \
         ON DUPLICATE KEY UPDATE \
           cost_usd = image_daily_spend.cost_usd + new.cost_usd, \
           generate_calls = image_daily_spend.generate_calls + new.generate_calls",
    )
    .bind(cost)
    .execute(&state.db)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_no_specific_garment_or_person() {
        let p = SIDE_PROMPT.to_ascii_lowercase();
        for word in [
            "burgundy",
            "denim",
            "knit",
            "jacket",
            "boots",
            "bob",
            "arcade",
            "leather",
            "indigo",
            "left shoulder",
            "smile",
        ] {
            assert!(
                !p.contains(word),
                "옆모습 문구에 특정 코디 묘사가 남았다: {word}"
            );
        }
        assert!(p.contains("90-degree profile"));
    }

    #[test]
    fn row_states_map_to_screen_states() {
        let row = |status: &str, path: Option<&str>, attempts| SideRow {
            status: status.to_string(),
            image_path: path.map(str::to_string),
            attempts,
        };
        assert_eq!(state_of(None).status, "none");
        assert_eq!(
            state_of(Some(&row("ready", Some("/static/images/a.png"), 1))),
            SideResponse::of("ready", Some("/static/images/a.png".into()), false)
        );
        assert!(state_of(Some(&row("failed", None, 1))).retryable);
        assert!(!state_of(Some(&row("failed", None, MAX_ATTEMPTS))).retryable);
        assert!(!state_of(Some(&row("blocked", None, 1))).retryable);
        assert_eq!(state_of(Some(&row("pending", None, 1))).status, "pending");
    }

    #[test]
    fn only_server_made_paths_count_as_files() {
        assert!(!source_file_exists("/static/images/../../.env"));
        assert!(!source_file_exists(
            "static/images/a7c6f9d9-55a8-4b5b-98f5-642cd38d334e.png"
        ));
        assert!(!source_file_exists("/static/images/not-a-uuid.png"));
    }
}

#[cfg(test)]
#[path = "lookbook_side_db_tests.rs"]
mod db_tests;
