//! 두 코디 비교(`services::outfit_comparison`).
//!
//!   POST /api/comparisons            {batch_id}                 — 비교를 열거나, 이미 있으면 그대로
//!   GET  /api/comparisons/{id}                                  — 내 비교(응답 포함)
//!   POST /api/comparisons/{id}/vote  {choice, reasons?, comment?} — 응답(먼저 쓴 것이 남는다)
//!
//! 모든 읽기·쓰기는 요청한 사용자(`AuthUser`)에 묶는다. 다른 사용자의 비교는 404 다.
//! 이 기능이 인증 자체를 바꾸지는 않는다 — 토큰이 없으면 지금처럼 `default` 사용자다.

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;
use crate::services::outfit_comparison::{
    self as cmp, ComparisonView, CreateError, VoteError, VoteOutcome, VoteRequest, VoteView,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create))
        .route("/{id}", get(show))
        .route("/{id}/vote", post(vote))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    batch_id: String,
}

/// 본문 해석 실패를 다른 오류와 같은 `{"error": ...}` 모양의 400 으로 돌려준다.
fn parse_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, AppError> {
    body.map(|Json(b)| b).map_err(|e| {
        AppError::BadRequest(format!("요청 형식이 올바르지 않습니다: {}", e.body_text()))
    })
}

/// id 는 서버가 만든 UUID 다. 모양이 다르면 DB 에 묻지 않고 404 로 끝낸다.
fn looks_like_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> Result<Json<ComparisonView>, AppError> {
    let body = parse_body(body)?;
    let not_found = || {
        AppError::NotFound(
            "이 추천을 찾을 수 없어요. 홈에서 코디를 다시 만들어 주세요.".to_string(),
        )
    };
    if !looks_like_id(&body.batch_id) {
        return Err(not_found());
    }
    // 좌우는 여기서 한 번만 정한다. 이미 비교가 있으면 이 값은 쓰이지 않는다.
    let a_first = uuid::Uuid::new_v4().as_bytes()[0] & 1 == 0;
    match cmp::create_comparison(&state.db, &auth.user_id, &body.batch_id, a_first).await {
        Ok(view) => Ok(Json(view)),
        Err(CreateError::BatchNotFound) => Err(not_found()),
        Err(CreateError::NotEnoughCandidates) => Err(AppError::Validation(
            "이번 추천에는 서로 다른 코디가 두 개 이상 없어 비교할 수 없어요.".to_string(),
        )),
        Err(CreateError::Db(e)) => Err(AppError::Database(e)),
    }
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ComparisonView>, AppError> {
    let not_found = || AppError::NotFound("이 비교를 찾을 수 없어요.".to_string());
    if !looks_like_id(&id) {
        return Err(not_found());
    }
    cmp::get_comparison(&state.db, &auth.user_id, &id)
        .await?
        .map(Json)
        .ok_or_else(not_found)
}

#[derive(Debug, Serialize)]
struct VoteResponse {
    vote: VoteView,
    /// 같은 응답이 이미 저장돼 있었다(다시 보내기·새로고침).
    already_submitted: bool,
}

async fn vote(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    body: Result<Json<VoteRequest>, JsonRejection>,
) -> Result<Json<VoteResponse>, AppError> {
    let not_found = || AppError::NotFound("이 비교를 찾을 수 없어요.".to_string());
    if !looks_like_id(&id) {
        return Err(not_found());
    }
    let vote = cmp::validate_vote(parse_body(body)?).map_err(|e| AppError::BadRequest(e.0))?;
    match cmp::submit_vote(&state.db, &auth.user_id, &id, &vote).await {
        Ok(VoteOutcome::Saved(vote)) => Ok(Json(VoteResponse {
            vote,
            already_submitted: false,
        })),
        Ok(VoteOutcome::AlreadySaved(vote)) => Ok(Json(VoteResponse {
            vote,
            already_submitted: true,
        })),
        Ok(VoteOutcome::Conflict) => Err(AppError::Conflict(
            "이 비교에는 이미 다른 선택이 저장돼 있어요. 처음 고른 것이 남아요.".to_string(),
        )),
        Err(VoteError::NotFound) => Err(not_found()),
        Err(VoteError::Db(e)) => Err(AppError::Database(e)),
    }
}

/// 실제 DB 로 HTTP 경로 전체를 잰다. `TEST_DATABASE_URL` 이 없으면 건너뛴다. 그 DB 에
/// 마이그레이션을 돌리므로 이름에 `test` 가 든 DB 에서만 돈다 — 운영 DB 를 가리키면 멈춘다.
/// 모델·이미지 API 는 부르지 않는다(추천 묶음은 직접 넣는다).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::outfit_comparison::{BatchContext, CandidateSnapshot, ItemSnapshot};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    struct Ctx {
        db: sqlx::MySqlPool,
        app: Router,
        /// (user_id, token)
        users: [(String, String); 2],
        run: String,
    }

    async fn setup() -> Option<Ctx> {
        let url = std::env::var("TEST_DATABASE_URL")
            .ok()
            .filter(|s| !s.is_empty())?;
        let db_name = url
            .rsplit('/')
            .next()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("");
        assert!(
            db_name.contains("test"),
            "TEST_DATABASE_URL 은 이름에 test 가 든 별도 DB 여야 합니다: {db_name}"
        );
        let db = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(10)
            .connect(&url)
            .await
            .expect("DB 연결");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("마이그레이션");

        let run = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let users = [0, 1].map(|i| (format!("cmp-test-{run}-{i}"), format!("tok-{run}-{i}")));
        for (id, token) in &users {
            sqlx::query("INSERT INTO app_user (id, username, api_token) VALUES (?, ?, ?)")
                .bind(id)
                .bind(id)
                .bind(token)
                .execute(&db)
                .await
                .expect("사용자");
        }

        let http = reqwest::Client::new();
        let llm = std::sync::Arc::new(crate::services::llm::LlmClient::from_env(http.clone()));
        let embedding = std::sync::Arc::new(
            crate::services::embedding::EmbeddingService::new(llm.clone()).expect("임베딩"),
        );
        let state = AppState {
            db: db.clone(),
            http_client: http,
            llm,
            kma_api_key: String::new(),
            embedding,
        };
        let app = Router::new()
            .nest("/api", crate::routes::api_router())
            .with_state(state);
        Some(Ctx {
            db,
            app,
            users,
            run,
        })
    }

    fn item(id: &str, name: &str, category: &str) -> ItemSnapshot {
        ItemSnapshot {
            id: id.to_string(),
            name: name.to_string(),
            category: category.to_string(),
            color: Some("블랙".to_string()),
            sub_category: None,
            material: None,
            has_image: false,
        }
    }

    fn cand(mode: &str, position: u8, items: Vec<ItemSnapshot>) -> CandidateSnapshot {
        CandidateSnapshot {
            mode: mode.to_string(),
            position,
            item_key: cmp::item_key(items.iter().map(|i| i.id.as_str())),
            items,
            final_score: 90,
            style_score: 90,
        }
    }

    /// 서로 다른 두 코디 + 첫 코디와 같은 구성(순서만 다름)이 든 묶음.
    async fn batch(ctx: &Ctx, user: usize, tag: &str) -> String {
        let p = format!("{}-{tag}", ctx.run);
        let t1 = item(&format!("{p}-t1"), "화이트 셔츠", "상의");
        let b1 = item(&format!("{p}-b1"), "데님 팬츠", "하의");
        let t2 = item(&format!("{p}-t2"), "네이비 니트", "상의");
        let candidates = vec![
            cand("todays_pick", 0, vec![t1.clone(), b1.clone()]),
            cand("variation", 1, vec![b1.clone(), t1.clone()]),
            cand("dormant_revival", 2, vec![t2, b1]),
        ];
        record(ctx, user, &candidates).await
    }

    async fn record(ctx: &Ctx, user: usize, candidates: &[CandidateSnapshot]) -> String {
        let bctx = BatchContext {
            gender: Some("female"),
            mood_key: Some("minimal"),
            occasion: Some("일상"),
            weather: serde_json::json!({"temperature": 14.5, "weather_description": "맑음"}),
            provenance: serde_json::json!({"prompt_version": "unknown"}),
        };
        cmp::record_batch(&ctx.db, &ctx.users[user].0, &bctx, candidates)
            .await
            .expect("묶음")
            .expect("묶음 id")
    }

    async fn call(
        ctx: &Ctx,
        method: &str,
        path: &str,
        user: Option<usize>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(u) = user {
            req = req.header("authorization", format!("Bearer {}", ctx.users[u].1));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = ctx.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    async fn open(ctx: &Ctx, user: usize, batch_id: &str) -> (StatusCode, serde_json::Value) {
        call(
            ctx,
            "POST",
            "/api/comparisons",
            Some(user),
            Some(serde_json::json!({ "batch_id": batch_id })),
        )
        .await
    }

    async fn vote_count(ctx: &Ctx, comparison_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM outfit_comparison_vote WHERE comparison_id = ?")
            .bind(comparison_id)
            .fetch_one(&ctx.db)
            .await
            .unwrap()
    }

    async fn cleanup(ctx: &Ctx) {
        for (u, _) in &ctx.users {
            for q in [
                "DELETE FROM outfit_comparison_vote WHERE user_id = ?",
                "DELETE FROM outfit_comparison WHERE user_id = ?",
                "DELETE FROM recommendation_batch WHERE user_id = ?",
                "DELETE FROM app_user WHERE id = ?",
            ] {
                sqlx::query(q).bind(u).execute(&ctx.db).await.unwrap();
            }
        }
        sqlx::query("DELETE FROM clothing WHERE id LIKE ?")
            .bind(format!("{}-%", ctx.run))
            .execute(&ctx.db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn comparison_flow_against_a_real_database() {
        let Some(ctx) = setup().await else {
            eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
            return;
        };

        // ── 비교 열기: 같은 구성(순서만 다른 코디)은 짝이 되지 않는다 ──
        let b = batch(&ctx, 0, "main").await;
        let (st, view) = open(&ctx, 0, &b).await;
        assert_eq!(st, StatusCode::OK, "{view}");
        let id = view["id"].as_str().unwrap().to_string();
        let keys: Vec<String> = view["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let ids: Vec<&str> = o["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i["id"].as_str().unwrap())
                    .collect();
                cmp::item_key(ids)
            })
            .collect();
        assert_ne!(keys[0], keys[1], "자기 자신과 비교하면 안 된다");
        assert!(view["vote"].is_null());
        // 모드 이름·점수·버전은 화면에 내려가지 않는다.
        let raw = view.to_string();
        assert!(!raw.contains("todays_pick") && !raw.contains("final_score"));
        assert!(!raw.contains("provenance"));

        // 새로고침·두 번 누르기: 같은 비교, 같은 좌우.
        let (_, again) = open(&ctx, 0, &b).await;
        assert_eq!(again["id"], view["id"]);
        assert_eq!(again["options"], view["options"]);
        let (st, shown) = call(
            &ctx,
            "GET",
            &format!("/api/comparisons/{id}"),
            Some(0),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(shown["options"], view["options"]);

        // ── 다른 사용자·토큰 없음(default)은 남의 비교를 못 본다 ──
        for user in [Some(1), None] {
            let (st, _) = call(&ctx, "GET", &format!("/api/comparisons/{id}"), user, None).await;
            assert_eq!(st, StatusCode::NOT_FOUND);
            let (st, _) = call(
                &ctx,
                "POST",
                &format!("/api/comparisons/{id}/vote"),
                user,
                Some(serde_json::json!({"choice": "a"})),
            )
            .await;
            assert_eq!(st, StatusCode::NOT_FOUND);
        }
        let (st, _) = open(&ctx, 1, &b).await;
        assert_eq!(
            st,
            StatusCode::NOT_FOUND,
            "남의 추천 묶음으로 비교를 열 수 없다"
        );

        // ── 잘못된 요청 ──
        let (st, _) = open(&ctx, 0, "not-a-uuid").await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, _) = open(&ctx, 0, &uuid::Uuid::new_v4().to_string()).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, err) = call(
            &ctx,
            "POST",
            "/api/comparisons",
            Some(0),
            Some(serde_json::json!({"batch_id": b, "user_id": ctx.users[1].0})),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "본문의 user_id 는 받지 않는다");
        assert!(err["error"].is_string());
        let vote_path = format!("/api/comparisons/{id}/vote");
        for bad in [
            serde_json::json!({"choice": "skip"}),
            serde_json::json!({"choice": "c"}),
            serde_json::json!({}),
            serde_json::json!({"choice": "a", "reasons": ["price"]}),
            serde_json::json!({"choice": "a", "comment": "가".repeat(201)}),
            serde_json::json!({"choice": "a", "item_ids": ["x"]}),
        ] {
            let (st, _) = call(&ctx, "POST", &vote_path, Some(0), Some(bad.clone())).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
        }
        // 형식이 깨진 JSON.
        let req = Request::builder()
            .method("POST")
            .uri(&vote_path)
            .header("authorization", format!("Bearer {}", ctx.users[0].1))
            .header("content-type", "application/json")
            .body(Body::from("{\"choice\":"))
            .unwrap();
        let res = ctx.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        // 건너뛰기·잘못된 요청은 응답 행을 남기지 않는다(노출 행은 열 때 이미 남았다).
        assert_eq!(vote_count(&ctx, &id).await, 0);

        // ── B 를 고르면 화면 자리와 실제 코디가 함께 남는다. 이유·메모는 선택 ──
        let (st, res) = call(
            &ctx,
            "POST",
            &vote_path,
            Some(0),
            Some(serde_json::json!({"choice": "b", "reasons": ["silhouette", "color_combo"], "comment": " 바지 핏이 좋아요 "})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{res}");
        assert_eq!(res["already_submitted"], false);
        assert_eq!(
            res["vote"]["reasons"],
            serde_json::json!(["color_combo", "silhouette"])
        );
        assert_eq!(res["vote"]["comment"], "바지 핏이 좋아요");
        let (choice, chosen_key, polarity): (String, String, String) = sqlx::query_as(
            "SELECT choice, chosen_item_key, reason_polarity FROM outfit_comparison_vote WHERE comparison_id = ?",
        )
        .bind(&id)
        .fetch_one(&ctx.db)
        .await
        .unwrap();
        assert_eq!((choice.as_str(), polarity.as_str()), ("b", "chosen"));
        assert_eq!(
            chosen_key, keys[1],
            "B 자리에 보인 코디가 고른 코디로 남는다"
        );

        // 같은 응답을 다시 보내면 그대로, 다른 응답은 409 — 처음 것이 남는다.
        let (st, res) = call(
            &ctx,
            "POST",
            &vote_path,
            Some(0),
            Some(serde_json::json!({"choice": "b", "reasons": ["color_combo", "silhouette"], "comment": "바지 핏이 좋아요"})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(res["already_submitted"], true);
        let (st, _) = call(
            &ctx,
            "POST",
            &vote_path,
            Some(0),
            Some(serde_json::json!({"choice": "a"})),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(vote_count(&ctx, &id).await, 1);
        let (_, shown) = call(
            &ctx,
            "GET",
            &format!("/api/comparisons/{id}"),
            Some(0),
            None,
        )
        .await;
        assert_eq!(shown["vote"]["choice"], "b");

        // ── 둘 다 아쉬움: 고른 코디 없이 '아쉬운 점' 으로 남는다. 이유 없이도 된다 ──
        let b2 = batch(&ctx, 0, "neither").await;
        let (_, v2) = open(&ctx, 0, &b2).await;
        let id2 = v2["id"].as_str().unwrap();
        let (st, _) = call(
            &ctx,
            "POST",
            &format!("/api/comparisons/{id2}/vote"),
            Some(0),
            Some(serde_json::json!({"choice": "neither", "reasons": ["weather_fit"]})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let (polarity, chosen): (String, Option<String>) = sqlx::query_as(
            "SELECT reason_polarity, chosen_item_key FROM outfit_comparison_vote WHERE comparison_id = ?",
        )
        .bind(id2)
        .fetch_one(&ctx.db)
        .await
        .unwrap();
        assert_eq!((polarity.as_str(), chosen), ("both_lacking", None));

        // ── A 를 이유 없이 ──
        let b3 = batch(&ctx, 0, "plain-a").await;
        let (_, v3) = open(&ctx, 0, &b3).await;
        let id3 = v3["id"].as_str().unwrap();
        let (st, res) = call(
            &ctx,
            "POST",
            &format!("/api/comparisons/{id3}/vote"),
            Some(0),
            Some(serde_json::json!({"choice": "a"})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(res["vote"]["reasons"], serde_json::json!([]));

        // ── 서로 다른 코디가 둘이 안 되면 422 ──
        let t = item(&format!("{}-solo-t", ctx.run), "흰 티", "상의");
        let solo = record(
            &ctx,
            0,
            &[
                cand("todays_pick", 0, vec![t.clone()]),
                cand("variation", 1, vec![t]),
            ],
        )
        .await;
        let (st, err) = open(&ctx, 0, &solo).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(err["error"].as_str().unwrap().contains("두 개"));

        // ── 동시에: 같은 묶음으로 여러 번 열어도 비교는 하나 ──
        let b4 = batch(&ctx, 0, "race").await;
        let opens = futures_join(8, |_| open(&ctx, 0, &b4)).await;
        let ids: std::collections::HashSet<String> = opens
            .iter()
            .map(|(st, v)| {
                assert_eq!(*st, StatusCode::OK);
                v["id"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(ids.len(), 1);
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM outfit_comparison WHERE batch_id = ?")
                .bind(&b4)
                .fetch_one(&ctx.db)
                .await
                .unwrap();
        assert_eq!(rows, 1);

        // 동시에 서로 다른 응답: 하나만 남고, 나머지는 같으면 200·다르면 409.
        let id4 = ids.into_iter().next().unwrap();
        let path4 = format!("/api/comparisons/{id4}/vote");
        let votes = futures_join(8, |i| {
            let choice = if i % 2 == 0 { "a" } else { "neither" };
            call(
                &ctx,
                "POST",
                &path4,
                Some(0),
                Some(serde_json::json!({ "choice": choice })),
            )
        })
        .await;
        assert_eq!(vote_count(&ctx, &id4).await, 1);
        let saved: String =
            sqlx::query_scalar("SELECT choice FROM outfit_comparison_vote WHERE comparison_id = ?")
                .bind(&id4)
                .fetch_one(&ctx.db)
                .await
                .unwrap();
        let fresh = votes
            .iter()
            .filter(|(st, v)| *st == StatusCode::OK && v["already_submitted"] == false)
            .count();
        assert_eq!(fresh, 1, "새로 저장된 응답은 하나뿐");
        for (i, (st, _)) in votes.iter().enumerate() {
            let mine = if i % 2 == 0 { "a" } else { "neither" };
            let expect = if mine == saved {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            };
            assert_eq!(*st, expect);
        }

        cleanup(&ctx).await;
    }

    /// 옷 이름이 바뀌거나 옷이 지워져도 보여 준 코디는 그때 그대로다. 좌우는 처음 정한 대로다.
    #[tokio::test]
    async fn snapshots_and_side_order_do_not_change_later() {
        let Some(ctx) = setup().await else {
            eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
            return;
        };
        let p = format!("{}-snap", ctx.run);
        for (id, name, cat) in [
            (format!("{p}-t1"), "화이트 셔츠", "상의"),
            (format!("{p}-t2"), "네이비 니트", "상의"),
            (format!("{p}-b1"), "데님 팬츠", "하의"),
        ] {
            sqlx::query(
                "INSERT INTO clothing (id, name, category, color) VALUES (?, ?, ?, '블랙')",
            )
            .bind(id)
            .bind(name)
            .bind(cat)
            .execute(&ctx.db)
            .await
            .unwrap();
        }
        let clothes = crate::db::clothing_repo::list_clothing(&ctx.db)
            .await
            .unwrap();
        let mode = |key: &str, ids: [&str; 2]| crate::models::recommendation::ModeRecommendation {
            mode: key.to_string(),
            mode_label: String::new(),
            mode_description: String::new(),
            outfit: ids
                .iter()
                .map(|id| {
                    let c = clothes
                        .iter()
                        .find(|c| c.id == format!("{p}-{id}"))
                        .unwrap();
                    crate::models::recommendation::OutfitItem {
                        id: c.id.clone(),
                        category: c.category.clone(),
                        name: c.name.clone(),
                        reason: String::new(),
                        image_url: None,
                        material: None,
                        wear: None,
                    }
                })
                .collect(),
            recommendation: String::new(),
            weather_summary: String::new(),
            tips: vec![],
            score: 80,
            verdict: String::new(),
            reason: String::new(),
            revival_items: vec![],
            scoring_detail: crate::models::recommendation::ScoringDetail {
                style_score: 80,
                recency_penalty: 0,
                diversity_bonus: 0,
                dormant_bonus: 0,
                weather_penalty: 0,
                final_score: 80,
            },
            shadow_score: None,
            silhouette_delta: None,
            mood_waiver: None,
            feedback_bonus: 0,
        };
        let modes = [
            mode("todays_pick", ["t1", "b1"]),
            mode("variation", ["t2", "b1"]),
        ];
        let snaps = cmp::snapshot_modes(&modes, &clothes);
        assert_eq!(snaps[0].items[0].color.as_deref(), Some("블랙"));
        let b = record(&ctx, 0, &snaps).await;

        // 좌우: 처음 만든 값(a_first=false → 뒤 순위가 A)이 남고, 다시 열 때 넘긴 값은 무시된다.
        let first = cmp::create_comparison(&ctx.db, &ctx.users[0].0, &b, false)
            .await
            .unwrap();
        assert_eq!(first.options[0].items[0].name, "네이비 니트");
        let again = cmp::create_comparison(&ctx.db, &ctx.users[0].0, &b, true)
            .await
            .unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.options[0].items, first.options[0].items);
        let higher: bool =
            sqlx::query_scalar("SELECT a_is_higher_ranked FROM outfit_comparison WHERE id = ?")
                .bind(&first.id)
                .fetch_one(&ctx.db)
                .await
                .unwrap();
        assert!(!higher);
        let representation: String = sqlx::query_scalar(
            "SELECT CAST(JSON_UNQUOTE(JSON_EXTRACT(context_json, '$.representation')) AS CHAR) \
             FROM outfit_comparison WHERE id = ?",
        )
        .bind(&first.id)
        .fetch_one(&ctx.db)
        .await
        .unwrap();
        assert_eq!(representation, cmp::REPRESENTATION);

        sqlx::query("UPDATE clothing SET name = '이름 바뀐 니트' WHERE id = ?")
            .bind(format!("{p}-t2"))
            .execute(&ctx.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM clothing WHERE id = ?")
            .bind(format!("{p}-b1"))
            .execute(&ctx.db)
            .await
            .unwrap();
        let later = cmp::get_comparison(&ctx.db, &ctx.users[0].0, &first.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(later.options[0].items, first.options[0].items);
        assert_eq!(later.options[1].items, first.options[1].items);
        assert_eq!(later.context.mood_key.as_deref(), Some("minimal"));
        assert_eq!(later.context.temperature, Some(14.5));

        cleanup(&ctx).await;
    }

    /// 같은 작업을 동시에 `n` 번 돌린다(추가 의존성 없이).
    async fn futures_join<'a, F, Fut>(n: usize, f: F) -> Vec<(StatusCode, serde_json::Value)>
    where
        F: Fn(usize) -> Fut,
        Fut: std::future::Future<Output = (StatusCode, serde_json::Value)> + 'a,
    {
        let futs: Vec<_> = (0..n).map(|i| Box::pin(f(i))).collect();
        let mut out = Vec::with_capacity(n);
        let mut pending: Vec<_> = futs.into_iter().map(Some).collect();
        let mut done: Vec<Option<(StatusCode, serde_json::Value)>> = vec![None; n];
        std::future::poll_fn(|cx| {
            let mut all = true;
            for (i, slot) in pending.iter_mut().enumerate() {
                if let Some(fut) = slot {
                    match fut.as_mut().poll(cx) {
                        std::task::Poll::Ready(v) => {
                            done[i] = Some(v);
                            *slot = None;
                        }
                        std::task::Poll::Pending => all = false,
                    }
                }
            }
            if all {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
        out.extend(done.into_iter().map(|v| v.unwrap()));
        out
    }
}
