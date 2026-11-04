//! 코디 흐름(참조·바꾸기·계획·착용)을 실제 DB 로 잰다.
//!
//! `TEST_DATABASE_URL` 이 없으면 건너뛴다(통과가 아니다 — "건너뜁니다"가 찍힌다). 이름에 `test` 가 든
//! DB 에서만 돈다. 모델·날씨 API 는 부르지 않는다(바꾸기는 코디에 저장된 기온을 쓴다).

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::AppState;
use crate::middleware::auth::AuthMode;
use crate::services::outfit_comparison::{BatchContext, CandidateSnapshot, ItemSnapshot, item_key};
use crate::services::outfit_flow as flow;
use crate::services::wear_history;

const DELEGATION_KEY: [u8; 32] = *b"test-delegation-key-0123456789ab";

struct Ctx {
    db: sqlx::MySqlPool,
    app: Router,
    run: String,
    /// (user_id, token)
    users: [(String, String); 2],
}

async fn setup() -> Option<Ctx> {
    let url = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let name = url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    assert!(
        name.contains("test"),
        "이름에 test 가 든 별도 DB 여야 합니다: {name}"
    );
    let db = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(20)
        .connect(&url)
        .await
        .expect("DB 연결");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("마이그레이션");
    let run = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let users = [0, 1].map(|i| (format!("flow-{run}-{i}"), format!("tok-flow-{run}-{i}")));
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
        auth_mode: AuthMode::Strict,
        admin_users: std::sync::Arc::new(Vec::new()),
        delegation_key: Some(std::sync::Arc::from(&DELEGATION_KEY[..])),
    };
    let app = Router::new()
        .nest("/api", crate::routes::api_router())
        .with_state(state);
    let ctx = Ctx {
        db,
        app,
        run,
        users,
    };
    for u in 0..2 {
        ctx.seed_wardrobe(u).await;
    }
    Some(ctx)
}

const SHOES_COMFY: &str = "shoe-sneaker";

struct Cloth<'a> {
    key: &'a str,
    category: &'a str,
    sub: Option<&'a str>,
    thickness: &'a str,
    tone: Option<&'a str>,
}

impl Ctx {
    fn id(&self, user: usize, key: &str) -> String {
        format!("{}-{user}-{key}", self.run)
    }

    fn user(&self, user: usize) -> &str {
        &self.users[user].0
    }

    async fn add(&self, user: usize, c: Cloth<'_>) {
        sqlx::query(
            "INSERT INTO clothing (id, user_id, name, category, sub_category, color, thickness, tone) \
             VALUES (?, ?, ?, ?, ?, '블랙', ?, ?)",
        )
        .bind(self.id(user, c.key))
        .bind(self.user(user))
        .bind(format!("{user}번 {}", c.key))
        .bind(c.category)
        .bind(c.sub)
        .bind(c.thickness)
        .bind(c.tone)
        .execute(&self.db)
        .await
        .expect("옷");
    }

    async fn seed_wardrobe(&self, user: usize) {
        for (key, category, sub, thickness, tone) in [
            ("top", "상의", None, "medium", Some("중간")),
            ("top-thick", "상의", None, "thick", Some("어두움")),
            ("top-thin", "상의", None, "thin", Some("밝음")),
            ("bottom", "하의", None, "medium", Some("어두움")),
            ("outer", "아우터", None, "medium", Some("어두움")),
            ("shoe-heel", "신발", Some("힐"), "medium", Some("어두움")),
            (
                SHOES_COMFY,
                "신발",
                Some("스니커즈"),
                "medium",
                Some("밝음"),
            ),
            (
                "shoe-loafer",
                "신발",
                Some("로퍼"),
                "medium",
                Some("어두움"),
            ),
            (
                "shoe-boots",
                "신발",
                Some("앵클부츠"),
                "medium",
                Some("어두움"),
            ),
            ("bag", "가방", None, "medium", Some("어두움")),
        ] {
            self.add(
                user,
                Cloth {
                    key,
                    category,
                    sub,
                    thickness,
                    tone,
                },
            )
            .await;
        }
    }

    fn snapshot(&self, user: usize, key: &str, category: &str) -> ItemSnapshot {
        ItemSnapshot {
            id: self.id(user, key),
            name: format!("{user}번 {key}"),
            category: category.to_string(),
            color: Some("블랙".to_string()),
            sub_category: None,
            material: None,
            has_image: false,
        }
    }

    /// 서버가 추천을 낸 것처럼 묶음 참조를 만든다.
    async fn outfit_ref(&self, user: usize, temperature: f64) -> String {
        self.outfit_ref_with(
            user,
            temperature,
            &[
                ("top", "상의"),
                ("bottom", "하의"),
                ("shoe-heel", "신발"),
                ("bag", "가방"),
            ],
        )
        .await
    }

    async fn outfit_ref_with(
        &self,
        user: usize,
        temperature: f64,
        items: &[(&str, &str)],
    ) -> String {
        let items: Vec<ItemSnapshot> = items
            .iter()
            .map(|(k, c)| self.snapshot(user, k, c))
            .collect();
        let batch = uuid::Uuid::new_v4().to_string();
        let cand = CandidateSnapshot {
            mode: "todays_pick".into(),
            position: 0,
            item_key: item_key(items.iter().map(|i| i.id.as_str())),
            items,
            final_score: 80,
            style_score: 80,
        };
        let ctx = BatchContext {
            gender: None,
            mood_key: None,
            occasion: Some("일상"),
            weather: json!({ "temperature": temperature, "weather_description": "맑음" }),
            provenance: json!({}),
        };
        flow::save_batch_refs(&self.db, self.user(user), &batch, &ctx, &[cand])
            .await
            .unwrap()
            .remove(0)
            .unwrap()
    }

    async fn post(&self, user: usize, path: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/outfits/{path}"))
            .header("authorization", format!("Bearer {}", self.users[user].1))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        send(&self.app, req).await
    }

    async fn wear_rows(&self, user: usize) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM outfit_wear WHERE user_id = ?")
            .bind(self.user(user))
            .fetch_one(&self.db)
            .await
            .unwrap()
    }

    async fn ref_rows(&self, user: usize) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM outfit_ref WHERE user_id = ?")
            .bind(self.user(user))
            .fetch_one(&self.db)
            .await
            .unwrap()
    }
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 22)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn ids(items: &Value) -> Vec<String> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

macro_rules! ctx {
    () => {
        match setup().await {
            Some(c) => c,
            None => {
                eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
                return;
            }
        }
    };
}

#[tokio::test]
async fn another_accounts_refs_and_records_look_missing() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let (st, planned) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(st, StatusCode::OK);
    let record = planned["record"]["record_id"].as_str().unwrap();

    // 참조: 있는 것을 다른 계정이 쓰면 없는 것과 똑같은 답이다.
    let (_, missing) = ctx
        .post(
            1,
            "plan",
            json!({ "outfit_ref": uuid::Uuid::new_v4().to_string() }),
        )
        .await;
    for (path, body) in [
        ("plan", json!({ "outfit_ref": mine })),
        ("worn", json!({ "outfit_ref": mine })),
        (
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발" }),
        ),
    ] {
        let (st, res) = ctx.post(1, path, body).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(res, missing, "{path}");
    }
    // 기록
    for (path, body) in [
        (
            "worn",
            json!({ "record_id": record, "expected_revision": 1 }),
        ),
        (
            "record",
            json!({ "record_id": record, "expected_revision": 1, "action": "cancel" }),
        ),
        (
            "record",
            json!({ "record_id": record, "expected_revision": 1, "action": "edit", "comment": "남의 메모" }),
        ),
    ] {
        let (st, res) = ctx.post(1, path, body).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(res["code"], "record_not_found");
    }
    // 내역은 본인 것만
    let (_, other) = ctx.post(1, "history", json!({})).await;
    assert!(other["records"].as_array().unwrap().is_empty());
    let (_, own) = ctx.post(0, "history", json!({})).await;
    assert_eq!(own["records"].as_array().unwrap().len(), 1);
    assert_eq!(ctx.wear_rows(1).await, 0);
    assert_eq!(
        own["records"][0]["revision"], 1,
        "다른 계정이 바꾸지 못했다"
    );
}

#[tokio::test]
async fn forged_clothing_and_bodies_are_refused() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let before = ctx.ref_rows(0).await;

    // 다른 계정의 옷, 없는 옷: 같은 답
    let theirs = ctx.id(1, SHOES_COMFY);
    let (st, a) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발", "replacement_item_id": theirs }),
        )
        .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(a["code"], "item_not_owned");
    let (_, b) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발", "replacement_item_id": "no-such-id" }),
        )
        .await;
    assert_eq!(a, b);

    // 다른 카테고리의 내 옷, 이미 코디에 든 옷
    let (st, c) = ctx
        .post(0, "replacement", json!({ "outfit_ref": mine, "category": "신발", "replacement_item_id": ctx.id(0, "top-thin") }))
        .await;
    assert_eq!(
        (st, c["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("wrong_category"))
    );
    let (_, d) = ctx
        .post(0, "replacement", json!({ "outfit_ref": mine, "category": "신발", "replacement_item_id": ctx.id(0, "shoe-heel") }))
        .await;
    assert_eq!(d["code"], "same_item");

    // 코디·사용자를 본문으로 지어 보낼 수 없다
    for (path, body) in [
        (
            "plan",
            json!({ "outfit_ref": mine, "items": [{"id": theirs, "name": "지어낸 옷"}] }),
        ),
        (
            "plan",
            json!({ "outfit_ref": mine, "user_id": ctx.user(1) }),
        ),
        ("worn", json!({ "outfit_ref": mine, "items": [] })),
    ] {
        let (st, _) = ctx.post(0, path, body).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
    }
    assert_eq!(ctx.wear_rows(0).await, 0);
    assert_eq!(ctx.ref_rows(0).await, before);
}

#[tokio::test]
async fn replacement_changes_one_slot_only_and_records_nothing() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let original = flow::get_ref(&ctx.db, ctx.user(0), &mine).await.unwrap();

    let (st, r) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발", "request_text": "편한 신발" }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "proposed");
    let alts = r["alternatives"].as_array().unwrap();
    // 편한 쪽은 스니커즈·로퍼뿐. 힐·부츠·남의 옷은 없다.
    let new_ids: Vec<&str> = alts
        .iter()
        .map(|a| a["new_item"]["id"].as_str().unwrap())
        .collect();
    let (sneaker, loafer) = (ctx.id(0, SHOES_COMFY), ctx.id(0, "shoe-loafer"));
    assert_eq!(new_ids.len(), 2, "{new_ids:?}");
    assert!(new_ids.contains(&sneaker.as_str()) && new_ids.contains(&loafer.as_str()));

    for alt in alts {
        let items: Vec<ItemSnapshot> = serde_json::from_value(alt["items"].clone()).unwrap();
        assert_eq!(items.len(), original.items.len());
        for (before, after) in original.items.iter().zip(&items) {
            if before.category == "신발" {
                assert_eq!(after.category, "신발");
                assert_ne!(after.id, before.id);
            } else {
                assert_eq!(after, before, "나머지 자리는 그대로");
            }
        }
        let rationale = alt["rationale"].to_string();
        assert!(rationale.contains("유형"), "{rationale}");
        assert!(rationale.contains("15°C"), "{rationale}");

        // 제안 자체로 저장된 참조 — 그 참조로 다시 읽으면 같은 구성이다.
        let saved = flow::get_ref(&ctx.db, ctx.user(0), alt["outfit_ref"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(saved.items, items);
    }

    // 원본은 그대로이고 기록은 생기지 않는다.
    let again = flow::get_ref(&ctx.db, ctx.user(0), &mine).await.unwrap();
    assert_eq!(again.items, original.items);
    assert_eq!(ctx.wear_rows(0).await, 0);

    // 같은 요청을 다시 보내면 같은 참조다(늘지 않는다).
    let refs_before = ctx.ref_rows(0).await;
    let (_, retry) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발", "preference": "comfortable" }),
        )
        .await;
    assert_eq!(retry["alternatives"], r["alternatives"]);
    assert_eq!(ctx.ref_rows(0).await, refs_before);

    // 고른 대안으로 입은 기록을 남길 수 있고, 스냅숏은 대안 그대로다.
    let chosen = alts[0]["outfit_ref"].as_str().unwrap();
    let (st, worn) = ctx.post(0, "worn", json!({ "outfit_ref": chosen })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(worn["record"]["items"], alts[0]["items"]);

    // 바꾼 코디에서 또 바꿀 수 있다(참조가 이어진다). 다른 자리는 그대로다.
    let (_, second) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": chosen, "category": "상의", "preference": "warmer" }),
        )
        .await;
    assert_eq!(second["outcome"], "proposed", "{second}");
    let items = ids(&second["alternatives"][0]["items"]);
    assert!(items.contains(&ctx.id(0, "top-thick")));
    assert!(items.contains(&alts[0]["new_item"]["id"].as_str().unwrap().to_string()));
}

#[tokio::test]
async fn unsupported_constraints_stop_instead_of_being_ignored() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let refs = ctx.ref_rows(0).await;
    for (body, reason) in [
        (
            json!({ "request_text": "비 오는 날 신을 방수 신발" }),
            "unsupported_constraint",
        ),
        (
            json!({ "request_text": "rainproof", "preference": "comfortable" }),
            "unsupported_constraint",
        ),
        (json!({ "request_text": "좀 힙하게" }), "ambiguous_request"),
        (json!({ "preference": "sparkly" }), "unsupported_preference"),
        (
            json!({ "preference": "warmer" }),
            "unsupported_for_category",
        ),
        (
            json!({ "preference": "darker", "replacement_item_id": ctx.id(0, SHOES_COMFY) }),
            "ambiguous_request",
        ),
    ] {
        let mut req = body.clone();
        req["outfit_ref"] = json!(mine);
        req["category"] = json!("신발");
        let (st, r) = ctx.post(0, "replacement", req).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert_eq!(r["outcome"], "needs_clarification", "{body}");
        assert_eq!(r["reason"], reason, "{body}");
        assert_eq!(r["alternatives"], json!([]));
    }
    // 없는 자리
    let (_, r) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "아우터" }),
        )
        .await;
    assert_eq!(r["reason"], "slot_empty");
    assert_eq!(ctx.ref_rows(0).await, refs, "되묻기는 참조를 만들지 않는다");
}

#[tokio::test]
async fn replacement_reuses_weather_gate_and_reports_no_candidates() {
    let ctx = ctx!();
    // 2°C: 샌들은 날씨에서 탈락한다. 직접 골라도 받지 않는다.
    let hot = ctx.outfit_ref(0, 2.0).await;
    ctx.add(
        0,
        Cloth {
            key: "shoe-sandal",
            category: "신발",
            sub: Some("샌들"),
            thickness: "thin",
            tone: None,
        },
    )
    .await;
    let (_, r) = ctx
        .post(0, "replacement", json!({ "outfit_ref": hot, "category": "신발", "replacement_item_id": ctx.id(0, "shoe-sandal") }))
        .await;
    assert_eq!(r["outcome"], "no_candidates", "{r}");
    assert_eq!(r["alternatives"], json!([]));
    let (_, r) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": hot, "category": "신발" }),
        )
        .await;
    let found = r["alternatives"].to_string();
    assert!(!found.contains("shoe-sandal"), "{found}");

    // 조건에 맞는 옷이 없으면 없다고 말한다.
    ctx.outfit_ref(0, 15.0).await;
    let thick = ctx
        .outfit_ref_with(0, 15.0, &[("top-thick", "상의"), ("bottom", "하의")])
        .await;
    let (_, r) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": thick, "category": "상의", "preference": "warmer" }),
        )
        .await;
    assert_eq!(r["outcome"], "no_candidates");
}

#[tokio::test]
async fn plan_then_worn_keeps_one_record_and_plan_is_not_wear() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let today = flow::today_kst();

    let (st, p) = ctx
        .post(
            0,
            "plan",
            json!({ "outfit_ref": mine, "comment": " 저녁 약속 " }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(p["outcome"], "planned");
    assert_eq!(p["record"]["status"], "planned");
    assert_eq!(p["record"]["date"], today.to_string());
    assert_eq!(p["record"]["comment"], "저녁 약속");
    assert!(p["record"]["worn_at"].is_null());

    // 계획은 입은 기록이 아니다.
    let recent = wear_history::load_recent(&ctx.db, ctx.user(0), today)
        .await
        .unwrap();
    assert!(recent.items.is_empty() && recent.outfits.is_empty());
    let (_, h) = ctx.post(0, "history", json!({ "status": "worn" })).await;
    assert!(h["records"].as_array().unwrap().is_empty());

    // 재시도 → 같은 행
    let (_, again) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(again["outcome"], "already_planned");
    assert_eq!(again["record"]["record_id"], p["record"]["record_id"]);

    // 참조로 입었다 → 같은 행이 확정된다
    let (_, w) = ctx
        .post(
            0,
            "worn",
            json!({ "outfit_ref": mine, "reaction": "comfortable" }),
        )
        .await;
    assert_eq!(w["outcome"], "worn");
    assert_eq!(w["record"]["record_id"], p["record"]["record_id"]);
    assert_eq!(w["record"]["revision"], 2);
    assert_eq!(w["record"]["reaction"], "comfortable");
    assert_eq!(w["record"]["comment"], "저녁 약속", "계획 메모가 남는다");
    assert_eq!(ctx.wear_rows(0).await, 1);

    // 재시도: 행도 값도 그대로
    let (_, retry) = ctx
        .post(
            0,
            "worn",
            json!({ "outfit_ref": mine, "reaction": "too_cold" }),
        )
        .await;
    assert_eq!(retry["outcome"], "already_worn");
    assert_eq!(retry["record"]["revision"], 2);
    assert_eq!(retry["record"]["reaction"], "comfortable");
    let (_, plan_after) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(
        plan_after["outcome"], "already_worn",
        "계획이 착용을 덮지 않는다"
    );

    let recent = wear_history::load_recent(&ctx.db, ctx.user(0), today)
        .await
        .unwrap();
    assert!(recent.items.contains_key(&ctx.id(0, "top")));
}

#[tokio::test]
async fn multiple_different_outfits_per_day_are_allowed() {
    let ctx = ctx!();
    let a = ctx.outfit_ref(0, 15.0).await;
    let b = ctx
        .outfit_ref_with(0, 15.0, &[("top-thin", "상의"), ("bottom", "하의")])
        .await;
    let (_, x) = ctx.post(0, "worn", json!({ "outfit_ref": a })).await;
    let (_, y) = ctx.post(0, "worn", json!({ "outfit_ref": b })).await;
    assert_eq!(
        (x["outcome"].as_str(), y["outcome"].as_str()),
        (Some("worn"), Some("worn"))
    );
    assert_ne!(x["record"]["record_id"], y["record"]["record_id"]);

    // 같은 구성이 다른 추천에서 다시 나와도 같은 날이면 한 행이다.
    let c = ctx.outfit_ref(0, 15.0).await;
    assert_ne!(a, c);
    let (_, z) = ctx.post(0, "worn", json!({ "outfit_ref": c })).await;
    assert_eq!(z["outcome"], "already_worn");
    assert_eq!(ctx.wear_rows(0).await, 2);
}

#[tokio::test]
async fn cancel_undo_and_revision_rules() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let today = flow::today_kst();
    let (_, p) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    let id = p["record"]["record_id"].as_str().unwrap().to_string();

    // 낡은 revision
    let (_, e) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 1, "action": "edit", "comment": "첫째" }),
        )
        .await;
    assert_eq!(e["outcome"], "updated");
    let (st, stale) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 1, "action": "edit", "comment": "둘째" }),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "revision_conflict");
    assert_eq!(stale["current_revision"], 2);
    // 같은 내용의 재시도는 충돌이 아니다.
    let (_, same) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 1, "action": "edit", "comment": "첫째" }),
        )
        .await;
    assert_eq!(same["outcome"], "unchanged");
    let (st, _) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 1, "action": "cancel" }),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // 반응은 입은 기록에만
    let (st, r) = ctx
        .post(0, "record", json!({ "record_id": id, "expected_revision": 2, "action": "edit", "reaction": "liked" }))
        .await;
    assert_eq!(
        (st, r["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("reaction_needs_worn"))
    );

    // 확정 → 되돌리기 → 계획으로
    let (_, w) = ctx
        .post(
            0,
            "worn",
            json!({ "record_id": id, "expected_revision": 2, "reaction": "too_cold" }),
        )
        .await;
    assert_eq!(w["outcome"], "worn");
    let (_, again) = ctx
        .post(
            0,
            "worn",
            json!({ "record_id": id, "expected_revision": 2 }),
        )
        .await;
    assert_eq!(again["outcome"], "already_worn", "확정 재시도");
    let (st, c) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 3, "action": "cancel" }),
        )
        .await;
    assert_eq!(
        (st, c["code"].as_str()),
        (StatusCode::CONFLICT, Some("invalid_transition"))
    );

    let (_, u) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 3, "action": "undo" }),
        )
        .await;
    assert_eq!(u["outcome"], "undone_to_planned");
    assert_eq!(u["record"]["status"], "planned");
    assert!(u["record"]["reaction"].is_null());
    assert!(u["record"]["worn_at"].is_null());
    assert_eq!(u["record"]["comment"], "첫째", "메모는 남는다");
    let recent = wear_history::load_recent(&ctx.db, ctx.user(0), today)
        .await
        .unwrap();
    assert!(recent.items.is_empty(), "되돌린 착용은 감점에서 빠진다");
    let (_, u2) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 3, "action": "undo" }),
        )
        .await;
    assert_eq!(u2["outcome"], "nothing_to_undo", "되돌리기 재시도");

    // 취소 → 재시도 → 확정 불가 → 다시 계획하면 새 행
    let (_, c) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 4, "action": "cancel" }),
        )
        .await;
    assert_eq!(c["outcome"], "cancelled");
    let (_, c2) = ctx
        .post(
            0,
            "record",
            json!({ "record_id": id, "expected_revision": 4, "action": "cancel" }),
        )
        .await;
    assert_eq!(c2["outcome"], "already_cancelled");
    let (st, _) = ctx
        .post(
            0,
            "worn",
            json!({ "record_id": id, "expected_revision": 5 }),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (_, fresh) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(fresh["outcome"], "planned");
    assert_ne!(fresh["record"]["record_id"], json!(id));

    // 바로 남긴 착용을 되돌리면 취소가 된다 — 계획이 있었던 척하지 않는다.
    let other = ctx
        .outfit_ref_with(0, 15.0, &[("top-thin", "상의"), ("bottom", "하의")])
        .await;
    let (_, d) = ctx.post(0, "worn", json!({ "outfit_ref": other })).await;
    let (_, u) = ctx
        .post(0, "record", json!({ "record_id": d["record"]["record_id"], "expected_revision": 1, "action": "undo" }))
        .await;
    assert_eq!(u["outcome"], "undone_to_cancelled");

    // 지금 남은 것은 살아 있는 계획 하나와 취소된 행들뿐 — 아무것도 착용 감점에 들어가지 않는다.
    let recent = wear_history::load_recent(&ctx.db, ctx.user(0), today)
        .await
        .unwrap();
    assert!(recent.items.is_empty() && recent.outfits.is_empty());
}

#[tokio::test]
async fn batch_refs_are_stable_for_the_same_result() {
    let ctx = ctx!();
    let item = ctx.snapshot(0, "top", "상의");
    let cand = CandidateSnapshot {
        mode: "todays_pick".into(),
        position: 1,
        item_key: item_key([item.id.as_str()]),
        items: vec![item],
        final_score: 80,
        style_score: 80,
    };
    let batch = uuid::Uuid::new_v4().to_string();
    let bc = BatchContext {
        gender: Some("female"),
        mood_key: Some("minimal"),
        occasion: Some("일상"),
        weather: json!({ "temperature": 15.0 }),
        provenance: json!({}),
    };
    let first = flow::save_batch_refs(
        &ctx.db,
        ctx.user(0),
        &batch,
        &bc,
        std::slice::from_ref(&cand),
    )
    .await
    .unwrap();
    let again = flow::save_batch_refs(&ctx.db, ctx.user(0), &batch, &bc, &[cand])
        .await
        .unwrap();
    assert_eq!(first, again);
    let saved = flow::get_ref(&ctx.db, ctx.user(0), first[0].as_ref().unwrap())
        .await
        .unwrap();
    assert_eq!(saved.context["mood_key"], "minimal");
    assert_eq!(saved.context["weather"]["temperature"], 15.0);
    // 같은 id 를 다른 계정이 읽으면 없다.
    assert!(
        flow::get_ref(&ctx.db, ctx.user(1), first[0].as_ref().unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dates_and_lengths_are_validated() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let today = flow::today_kst();
    let day = |n: i64| (today + chrono::Duration::days(n)).to_string();
    for (path, date, code) in [
        ("plan", "2026-13-01".to_string(), "invalid_date"),
        ("plan", "2026-2-30".to_string(), "invalid_date"),
        ("plan", "오늘".to_string(), "invalid_date"),
        ("plan", day(-1), "date_in_past"),
        ("plan", day(61), "date_too_far"),
        ("worn", day(1), "date_in_future"),
        ("worn", day(-366), "date_too_old"),
    ] {
        let (st, r) = ctx
            .post(0, path, json!({ "outfit_ref": mine, "date": date }))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{path} {date}");
        assert_eq!(r["code"], code, "{path} {date}");
        assert!(!r["error"].as_str().unwrap().is_ascii(), "한국어 문장");
    }
    let (_, r) = ctx
        .post(
            0,
            "plan",
            json!({ "outfit_ref": mine, "comment": "가".repeat(201) }),
        )
        .await;
    assert_eq!(r["code"], "comment_too_long");
    let (_, r) = ctx
        .post(
            0,
            "worn",
            json!({ "outfit_ref": mine, "reaction": "great" }),
        )
        .await;
    assert_eq!(r["code"], "invalid_reaction");
    let (_, r) = ctx
        .post(0, "worn", json!({ "outfit_ref": mine, "record_id": "x" }))
        .await;
    assert_eq!(r["code"], "invalid_request");
    let (_, r) = ctx
        .post(0, "history", json!({ "from": day(1), "to": day(0) }))
        .await;
    assert_eq!(r["code"], "invalid_range");
    assert_eq!(ctx.wear_rows(0).await, 0);

    // 미래 계획을 그대로 확정할 수는 없고, 날짜를 밝혀야 한다.
    let (_, p) = ctx
        .post(0, "plan", json!({ "outfit_ref": mine, "date": day(3) }))
        .await;
    let id = p["record"]["record_id"].clone();
    let (st, r) = ctx
        .post(
            0,
            "worn",
            json!({ "record_id": id, "expected_revision": 1 }),
        )
        .await;
    assert_eq!(
        (st, r["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("date_in_future"))
    );
    let (_, r) = ctx
        .post(
            0,
            "worn",
            json!({ "record_id": id, "expected_revision": 1, "date": day(0) }),
        )
        .await;
    assert_eq!(r["record"]["date"], day(0));

    // 지난 날짜의 입은 기록은 남길 수 있고 내역 범위로 찾는다.
    let b = ctx
        .outfit_ref_with(0, 15.0, &[("top-thin", "상의"), ("bottom", "하의")])
        .await;
    let (_, r) = ctx
        .post(0, "worn", json!({ "outfit_ref": b, "date": day(-2) }))
        .await;
    assert_eq!(r["outcome"], "worn");
    let (_, h) = ctx
        .post(0, "history", json!({ "from": day(-2), "to": day(-2) }))
        .await;
    assert_eq!(h["records"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn deleted_clothes_block_new_records_but_keep_old_snapshots() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let other = ctx
        .outfit_ref_with(0, 15.0, &[("top-thin", "상의"), ("bottom", "하의")])
        .await;
    let (_, w) = ctx.post(0, "worn", json!({ "outfit_ref": other })).await;
    sqlx::query("DELETE FROM clothing WHERE id = ?")
        .bind(ctx.id(0, "bottom"))
        .execute(&ctx.db)
        .await
        .unwrap();

    let (st, r) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(
        (st, r["code"].as_str()),
        (StatusCode::CONFLICT, Some("item_gone"))
    );
    let (st, r) = ctx
        .post(
            0,
            "replacement",
            json!({ "outfit_ref": mine, "category": "신발" }),
        )
        .await;
    assert_eq!(
        (st, r["code"].as_str()),
        (StatusCode::CONFLICT, Some("item_gone"))
    );

    let (_, h) = ctx.post(0, "history", json!({})).await;
    assert_eq!(
        h["records"][0]["items"], w["record"]["items"],
        "스냅숏은 값으로 남는다"
    );
}

#[tokio::test]
async fn concurrent_retries_make_one_record() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let ctx = std::sync::Arc::new(ctx);

    // 같은 계획 12번 동시에
    let tasks: Vec<_> = (0..12)
        .map(|_| {
            let (c, r) = (ctx.clone(), mine.clone());
            tokio::spawn(async move { c.post(0, "plan", json!({ "outfit_ref": r })).await })
        })
        .collect();
    let mut created = 0;
    for t in tasks {
        let (st, r) = t.await.unwrap();
        assert_eq!(st, StatusCode::OK, "{r}");
        created += i32::from(r["outcome"] == "planned");
    }
    assert_eq!(created, 1);
    assert_eq!(ctx.wear_rows(0).await, 1);

    // 입었다 12번 동시에
    let tasks: Vec<_> = (0..12)
        .map(|_| {
            let (c, r) = (ctx.clone(), mine.clone());
            tokio::spawn(async move { c.post(0, "worn", json!({ "outfit_ref": r })).await })
        })
        .collect();
    let mut worn = 0;
    for t in tasks {
        let (st, r) = t.await.unwrap();
        assert_eq!(st, StatusCode::OK, "{r}");
        worn += i32::from(r["outcome"] == "worn");
    }
    assert_eq!(worn, 1);
    assert_eq!(ctx.wear_rows(0).await, 1);
    let rev: i32 = sqlx::query_scalar("SELECT revision FROM outfit_wear WHERE user_id = ?")
        .bind(ctx.user(0))
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(rev, 2);

    // 바로 남기기(계획 없음) 동시에
    let direct = ctx
        .outfit_ref_with(0, 15.0, &[("top-thin", "상의"), ("bottom", "하의")])
        .await;
    let tasks: Vec<_> = (0..12)
        .map(|_| {
            let (c, r) = (ctx.clone(), direct.clone());
            tokio::spawn(async move { c.post(0, "worn", json!({ "outfit_ref": r })).await })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap().0, StatusCode::OK);
    }
    assert_eq!(ctx.wear_rows(0).await, 2);
}

#[tokio::test]
async fn confirm_and_cancel_race_has_one_winner() {
    let ctx = std::sync::Arc::new(ctx!());
    for round in 0..8 {
        let r = ctx
            .outfit_ref_with(0, 15.0, &[("top", "상의"), ("bottom", "하의")])
            .await;
        let date = (flow::today_kst() + chrono::Duration::days(round)).to_string();
        let (_, p) = ctx
            .post(0, "plan", json!({ "outfit_ref": r, "date": date }))
            .await;
        let id = p["record"]["record_id"].as_str().unwrap().to_string();
        let today = flow::today_kst().to_string();
        let a = {
            let (c, id, today) = (ctx.clone(), id.clone(), today.clone());
            tokio::spawn(async move {
                c.post(
                    0,
                    "worn",
                    json!({ "record_id": id, "expected_revision": 1, "date": today }),
                )
                .await
            })
        };
        let b = {
            let (c, id) = (ctx.clone(), id.clone());
            tokio::spawn(async move {
                c.post(
                    0,
                    "record",
                    json!({ "record_id": id, "expected_revision": 1, "action": "cancel" }),
                )
                .await
            })
        };
        let ((sa, ra), (sb, rb)) = (a.await.unwrap(), b.await.unwrap());
        // 하나만 성공한다. 진 쪽은 충돌 또는 전이 오류이고, 행은 이긴 쪽 상태다.
        let wins = i32::from(sa == StatusCode::OK) + i32::from(sb == StatusCode::OK);
        assert_eq!(wins, 1, "{ra} {rb}");
        let (status, rev): (String, i32) =
            sqlx::query_as("SELECT status, revision FROM outfit_wear WHERE id = ?")
                .bind(&id)
                .fetch_one(&ctx.db)
                .await
                .unwrap();
        assert_eq!(rev, 2);
        assert_eq!(
            status,
            if sa == StatusCode::OK {
                "worn"
            } else {
                "cancelled"
            }
        );
        // 다음 라운드의 오늘 날짜와 겹치지 않게 지운다(일회용 DB 의 이 테스트 행).
        sqlx::query("DELETE FROM outfit_wear WHERE id = ?")
            .bind(&id)
            .execute(&ctx.db)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn legacy_worn_flag_is_not_wear_history() {
    let ctx = ctx!();
    let id = uuid::Uuid::new_v4().to_string();
    let outfit = json!([{ "id": ctx.id(0, "top"), "name": "지어낸 이름", "category": "상의" }, { "id": ctx.id(0, "bottom"), "name": "b", "category": "하의" }]);
    sqlx::query(
        "INSERT INTO lookbook_look (id, user_id, title, image_prompt, outfit_json, worn) \
         VALUES (?, ?, '옛 코디', ?, ?, 1)",
    )
    .bind(&id)
    .bind(ctx.user(0))
    .bind(&id)
    .bind(&outfit)
    .execute(&ctx.db)
    .await
    .unwrap();

    // `worn` 표시는 어디에도 입은 기록으로 나타나지 않는다.
    let (_, h) = ctx.post(0, "history", json!({})).await;
    assert!(h["records"].as_array().unwrap().is_empty());
    let today = flow::today_kst();
    assert!(
        wear_history::load_recent(&ctx.db, ctx.user(0), today)
            .await
            .unwrap()
            .items
            .is_empty()
    );

    // 룩에서 참조: 이름은 옷장 값이고, 다른 계정은 열 수 없다.
    let (st, _) = ctx.post(1, "from-look", json!({ "look_id": id })).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, r) = ctx.post(0, "from-look", json!({ "look_id": id })).await;
    let outfit_ref = r["outfit_ref"].as_str().unwrap();
    let saved = flow::get_ref(&ctx.db, ctx.user(0), outfit_ref)
        .await
        .unwrap();
    assert_eq!(saved.items[0].name, "0번 top");
    let (_, again) = ctx.post(0, "from-look", json!({ "look_id": id })).await;
    assert_eq!(again["outfit_ref"], r["outfit_ref"]);

    // 옷 id 없는 옛 룩, 남의 옷 id 를 넣은 룩은 참조가 되지 않는다.
    for (outfit, code, status) in [
        (
            json!([{ "name": "이름만", "category": "상의" }]),
            "legacy_look",
            StatusCode::CONFLICT,
        ),
        (
            json!([{ "id": ctx.id(1, "top"), "name": "남의 옷", "category": "상의" }]),
            "item_gone",
            StatusCode::CONFLICT,
        ),
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO lookbook_look (id, user_id, title, image_prompt, outfit_json) VALUES (?, ?, 't', ?, ?)")
            .bind(&id).bind(ctx.user(0)).bind(&id).bind(&outfit).execute(&ctx.db).await.unwrap();
        let (st, r) = ctx.post(0, "from-look", json!({ "look_id": id })).await;
        assert_eq!((st, r["code"].as_str()), (status, Some(code)));
    }
}

#[tokio::test]
async fn delegation_reaches_fixed_paths_only_with_the_signed_user() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let exp = chrono::Utc::now().timestamp() + 60;
    let call = |path: &str, signed_path: &str, user: &str, body: Value| {
        let header =
            crate::middleware::delegation::sign(&DELEGATION_KEY, "POST", signed_path, user, exp);
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:3003")
            .header("x-wardrobe-delegation", header)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:55000".parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        req
    };
    let (st, r) = send(
        &ctx.app,
        call(
            "/api/outfits/plan",
            "/api/outfits/plan",
            ctx.user(0),
            json!({ "outfit_ref": mine }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "planned");
    // 서명된 사용자가 다르면 그 사용자의 소유 검사를 받는다.
    let (st, _) = send(
        &ctx.app,
        call(
            "/api/outfits/plan",
            "/api/outfits/plan",
            ctx.user(1),
            json!({ "outfit_ref": mine }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // 허용 경로 밖(웹 전용 경로)은 서명이 맞아도 401.
    let (st, _) = send(
        &ctx.app,
        call(
            "/api/outfits/from-look",
            "/api/outfits/from-look",
            ctx.user(0),
            json!({ "look_id": "x" }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // 다른 경로의 서명은 쓸 수 없다.
    let (st, _) = send(
        &ctx.app,
        call(
            "/api/outfits/worn",
            "/api/outfits/plan",
            ctx.user(0),
            json!({ "outfit_ref": mine }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(ctx.wear_rows(0).await, 1);
}

#[tokio::test]
async fn migration_can_run_again_without_touching_rows() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    let sql = include_str!("../../migrations/20261104000001_create_outfit_wear.sql");
    sqlx::raw_sql(sql)
        .execute(&ctx.db)
        .await
        .expect("다시 실행");
    assert_eq!(ctx.wear_rows(0).await, 1);
    // 유일 키는 그대로다(취소된 행은 키에 걸리지 않는다).
    let (_, h) = ctx.post(0, "history", json!({})).await;
    let id = h["records"][0]["record_id"].clone();
    ctx.post(
        0,
        "record",
        json!({ "record_id": id, "expected_revision": 1, "action": "cancel" }),
    )
    .await;
    let (_, p) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    assert_eq!(p["outcome"], "planned");
    assert_eq!(ctx.wear_rows(0).await, 2);
}

async fn add_look(
    ctx: &Ctx,
    user: usize,
    mood: Option<&str>,
    keys: &[&str],
    image: Option<&str>,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let outfit: Vec<Value> = keys
        .iter()
        .map(|k| json!({ "id": ctx.id(user, k), "name": "룩의 이름", "category": "상의" }))
        .collect();
    sqlx::query(
        "INSERT INTO lookbook_look (id, user_id, mood_key, title, image_prompt, outfit_json, image_path) \
         VALUES (?, ?, ?, '룩 제목', ?, ?, ?)",
    )
    .bind(&id)
    .bind(ctx.user(user))
    .bind(mood)
    .bind(&id)
    .bind(json!(outfit))
    .bind(image)
    .execute(&ctx.db)
    .await
    .unwrap();
    id
}

async fn own_image(ctx: &Ctx, user: usize, path: &str) {
    sqlx::query(
        "INSERT INTO outfit_image (id, user_id, outfit_hash, prompt_hash, image_path) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(ctx.user(user))
    .bind(uuid::Uuid::new_v4().simple().to_string())
    .bind(uuid::Uuid::new_v4().simple().to_string())
    .bind(path)
    .execute(&ctx.db)
    .await
    .unwrap();
}

/// `static/images` 에 테스트 PNG 를 잠깐 둔다. 끝나면 지운다.
struct TempImage {
    path: String,
    file: std::path::PathBuf,
}

impl TempImage {
    fn new() -> Self {
        let name = format!("{}.png", uuid::Uuid::new_v4());
        let file = std::path::Path::new("static/images").join(&name);
        std::fs::create_dir_all("static/images").unwrap();
        std::fs::write(&file, b"\x89PNG\r\n\x1a\n").unwrap();
        Self {
            path: format!("/static/images/{name}"),
            file,
        }
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

const TWO: [(&str, &str); 2] = [("top", "상의"), ("bottom", "하의")];

#[tokio::test]
async fn detail_opens_saved_outfit_in_every_status_for_its_owner_only() {
    let ctx = ctx!();
    // 사진도 룩도 없어도 저장된 구성은 열린다.
    let r = ctx.outfit_ref_with(0, 15.0, &TWO).await;
    let (_, p) = ctx.post(0, "plan", json!({ "outfit_ref": r })).await;
    let id = p["record"]["record_id"].clone();
    let check = |d: &Value, status: &str| {
        assert_eq!(d["record"]["status"], status);
        assert_eq!(d["record"]["items"], p["record"]["items"]);
        assert_eq!(d["image"]["status"], "none");
        assert_eq!(d["look_status"], "none");
        assert!(d["image"]["file"].is_null());
    };
    let (st, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    check(&d, "planned");
    ctx.post(
        0,
        "worn",
        json!({ "record_id": id, "expected_revision": 1 }),
    )
    .await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    check(&d, "worn");
    ctx.post(
        0,
        "record",
        json!({ "record_id": id, "expected_revision": 2, "action": "undo" }),
    )
    .await;
    ctx.post(
        0,
        "record",
        json!({ "record_id": id, "expected_revision": 3, "action": "cancel" }),
    )
    .await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    check(&d, "cancelled");

    // 다른 계정은 없는 기록과 같은 답이다.
    let (st, other) = ctx.post(1, "detail", json!({ "record_id": id })).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, missing) = ctx
        .post(
            1,
            "detail",
            json!({ "record_id": uuid::Uuid::new_v4().to_string() }),
        )
        .await;
    assert_eq!(other, missing);
    assert_eq!(other["code"], "record_not_found");

    // 읽기만 한다.
    let (_, h) = ctx.post(0, "history", json!({})).await;
    assert_eq!(h["records"][0]["revision"], 4);
}

async fn record_of(ctx: &Ctx, look: Option<&str>) -> Value {
    let r = ctx.outfit_ref_with(0, 15.0, &TWO).await;
    let mut body = json!({ "outfit_ref": r });
    if let Some(l) = look {
        body["look_id"] = json!(l);
    }
    let (st, p) = ctx.post(0, "plan", body).await;
    assert_eq!(st, StatusCode::OK, "{p}");
    p["record"]["record_id"].clone()
}

async fn set_look_image(ctx: &Ctx, look: &str, path: Option<&str>) {
    sqlx::query("UPDATE lookbook_look SET image_path = ? WHERE id = ?")
        .bind(path)
        .bind(look)
        .execute(&ctx.db)
        .await
        .unwrap();
}

fn file_of(image: &TempImage) -> String {
    image.path.trim_start_matches("/static/images/").to_string()
}

#[tokio::test]
async fn photo_stays_pinned_through_new_looks_changes_and_every_transition() {
    let ctx = ctx!();
    let first = TempImage::new();
    own_image(&ctx, 0, &first.path).await;
    let look = add_look(&ctx, 0, None, &["top", "bottom"], Some(&first.path)).await;
    let id = record_of(&ctx, Some(&look)).await;
    let pinned = |d: &Value| {
        assert_eq!(d["look"]["id"], look.as_str(), "{d}");
        assert_eq!(d["image"]["status"], "available", "{d}");
        assert_eq!(d["image"]["file"], file_of(&first), "{d}");
        assert_eq!(d["image"]["basis"], "at_save", "{d}");
    };
    pinned(&ctx.post(0, "detail", json!({ "record_id": id })).await.1);

    // 나중에 같은 구성의 룩이 생기고, 원래 룩의 사진이 바뀌어도 그대로다.
    let later = TempImage::new();
    own_image(&ctx, 0, &later.path).await;
    let newer = add_look(&ctx, 0, None, &["bottom", "top"], Some(&later.path)).await;
    set_look_image(&ctx, &look, Some(&later.path)).await;
    pinned(&ctx.post(0, "detail", json!({ "record_id": id })).await.1);

    // 재요청은 다른 룩을 보내도 먼저 고정한 것을 바꾸지 못한다.
    let r = sqlx::query_scalar::<_, String>("SELECT outfit_ref_id FROM outfit_wear WHERE id = ?")
        .bind(id.as_str().unwrap())
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    let (_, again) = ctx
        .post(0, "plan", json!({ "outfit_ref": r, "look_id": newer }))
        .await;
    assert_eq!(again["outcome"], "already_planned");
    pinned(&ctx.post(0, "detail", json!({ "record_id": id })).await.1);

    // 수정·확정·되돌리기·취소에도 그대로다. revision 은 사용자 변경만큼만 오른다.
    for (path, body, rev) in [
        (
            "record",
            json!({ "record_id": id, "expected_revision": 1, "action": "edit", "comment": "메모" }),
            2,
        ),
        (
            "worn",
            json!({ "record_id": id, "expected_revision": 2 }),
            3,
        ),
        (
            "record",
            json!({ "record_id": id, "expected_revision": 3, "action": "undo" }),
            4,
        ),
        (
            "record",
            json!({ "record_id": id, "expected_revision": 4, "action": "cancel" }),
            5,
        ),
    ] {
        let (st, r) = ctx.post(0, path, body).await;
        assert_eq!(st, StatusCode::OK, "{r}");
        let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
        pinned(&d);
        assert_eq!(d["record"]["revision"], rev);
    }
}

#[tokio::test]
async fn record_without_a_pin_never_borrows_a_similar_look() {
    let ctx = ctx!();
    // ChatGPT 처럼 룩 없이 저장한 기록. 구성이 같은 룩이 있어도 조회가 연결하지 않는다.
    let image = TempImage::new();
    own_image(&ctx, 0, &image.path).await;
    add_look(&ctx, 0, None, &["top", "bottom"], Some(&image.path)).await;
    let id = record_of(&ctx, None).await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["look_status"], "none", "{d}");
    assert!(d["look"].is_null());
    assert_eq!(d["image"]["status"], "none");
}

#[tokio::test]
async fn foreign_or_different_looks_cannot_be_pinned() {
    let ctx = ctx!();
    let r = ctx.outfit_ref_with(0, 15.0, &TWO).await;
    let plan = |look: String| {
        let r = r.clone();
        let ctx = &ctx;
        async move {
            ctx.post(0, "plan", json!({ "outfit_ref": r, "look_id": look }))
                .await
        }
    };

    // 다른 계정의 룩은 없는 룩과 같은 답이다.
    let theirs = add_look(&ctx, 1, None, &["top", "bottom"], None).await;
    let (st, a) = plan(theirs).await;
    assert_eq!(
        (st, a["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("look_not_found"))
    );
    let (_, b) = plan(uuid::Uuid::new_v4().to_string()).await;
    assert_eq!(a, b);

    // 옷이 다른 내 룩, 옷 id 가 빠진 예전 룩
    let other = add_look(&ctx, 0, None, &["top", "shoe-heel"], None).await;
    let (st, c) = plan(other).await;
    assert_eq!(
        (st, c["code"].as_str()),
        (StatusCode::CONFLICT, Some("look_mismatch"))
    );
    let legacy = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO lookbook_look (id, user_id, title, image_prompt, outfit_json) VALUES (?, ?, 't', ?, ?)",
    )
    .bind(&legacy)
    .bind(ctx.user(0))
    .bind(&legacy)
    .bind(json!([{ "name": "이름만", "category": "상의" }, { "name": "이름만2", "category": "하의" }]))
    .execute(&ctx.db)
    .await
    .unwrap();
    let (st, _) = plan(legacy).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(ctx.wear_rows(0).await, 0, "거절되면 기록도 없다");

    // 룩북 참조에 다른 룩을 보내면 거절한다.
    let own = add_look(&ctx, 0, None, &["top", "bottom"], None).await;
    let twin = add_look(&ctx, 0, None, &["bottom", "top"], None).await;
    let (_, from) = ctx.post(0, "from-look", json!({ "look_id": own })).await;
    let (st, d) = ctx
        .post(
            0,
            "plan",
            json!({ "outfit_ref": from["outfit_ref"], "look_id": twin }),
        )
        .await;
    assert_eq!(
        (st, d["code"].as_str()),
        (StatusCode::CONFLICT, Some("look_mismatch"))
    );

    // 내 룩에 다른 계정의 사진 경로가 붙어 있으면 룩만 연결하고 사진은 고정하지 않는다.
    let foreign = TempImage::new();
    own_image(&ctx, 1, &foreign.path).await;
    set_look_image(&ctx, &own, Some(&foreign.path)).await;
    let (_, p) = plan(own).await;
    let (_, d) = ctx
        .post(
            0,
            "detail",
            json!({ "record_id": p["record"]["record_id"] }),
        )
        .await;
    assert_eq!(d["look_status"], "linked");
    assert_eq!(d["image"]["status"], "none", "{d}");
    assert!(!d.to_string().contains(&file_of(&foreign)));
}

#[tokio::test]
async fn photo_made_later_comes_only_from_the_pinned_look_and_then_stays() {
    let ctx = ctx!();
    let look = add_look(&ctx, 0, None, &["top", "bottom"], None).await;
    let id = record_of(&ctx, Some(&look)).await;

    // 같은 구성의 다른 룩에 사진이 있어도 쓰지 않는다.
    let other = TempImage::new();
    own_image(&ctx, 0, &other.path).await;
    add_look(&ctx, 0, None, &["bottom", "top"], Some(&other.path)).await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(
        (d["look_status"].as_str(), d["image"]["status"].as_str()),
        (Some("linked"), Some("none"))
    );

    // 고정된 룩에 사진이 생기면 그 사진을 이어 받고, 그 뒤로는 바뀌지 않는다.
    let first = TempImage::new();
    own_image(&ctx, 0, &first.path).await;
    set_look_image(&ctx, &look, Some(&first.path)).await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["image"]["file"], file_of(&first), "{d}");
    assert_eq!(d["image"]["basis"], "later");
    assert_eq!(d["record"]["revision"], 1, "사용자 변경이 아니다");
    set_look_image(&ctx, &look, Some(&other.path)).await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["image"]["file"], file_of(&first));
    assert_eq!(d["image"]["basis"], "later");
}

#[tokio::test]
async fn detail_is_honest_about_missing_photos_and_clothes() {
    let ctx = ctx!();
    // 고정한 사진의 파일이 없다.
    let gone = format!("/static/images/{}.png", uuid::Uuid::new_v4());
    own_image(&ctx, 0, &gone).await;
    let look = add_look(&ctx, 0, None, &["top", "bottom"], Some(&gone)).await;
    let id = record_of(&ctx, Some(&look)).await;
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["image"]["status"], "file_missing");
    assert!(d["image"]["file"].is_null());

    // 고정한 경로가 더는 내 사진이 아니면 내보내지 않는다.
    let image = TempImage::new();
    own_image(&ctx, 0, &image.path).await;
    sqlx::query("UPDATE outfit_wear SET pinned_image_path = ? WHERE id = ?")
        .bind(&image.path)
        .bind(id.as_str().unwrap())
        .execute(&ctx.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM outfit_image WHERE image_path = ?")
        .bind(&image.path)
        .execute(&ctx.db)
        .await
        .unwrap();
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["image"]["status"], "file_missing", "{d}");
    assert!(!d.to_string().contains(&file_of(&image)));

    // 옷장에서 지운 옷은 스냅숏으로 남고 지워졌다고 알린다.
    sqlx::query("DELETE FROM clothing WHERE id = ?")
        .bind(ctx.id(0, "bottom"))
        .execute(&ctx.db)
        .await
        .unwrap();
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["removed_item_ids"], json!([ctx.id(0, "bottom")]));
    assert_eq!(d["record"]["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn lookbook_records_pin_their_look_and_keep_the_photo_after_deletion() {
    let ctx = ctx!();
    let image = TempImage::new();
    own_image(&ctx, 0, &image.path).await;
    let look = add_look(&ctx, 0, None, &["top", "bottom"], Some(&image.path)).await;
    let (_, r) = ctx.post(0, "from-look", json!({ "look_id": look })).await;
    // 룩북 참조는 바로 입었다고 남겨도 그 룩이 고정된다.
    let (_, w) = ctx
        .post(0, "worn", json!({ "outfit_ref": r["outfit_ref"] }))
        .await;
    let id = w["record"]["record_id"].clone();
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["source"], "lookbook");
    assert_eq!(d["look"]["id"], look.as_str());
    assert_eq!(d["image"]["basis"], "at_save");

    // 룩을 지우면 그렇다고 알리되 고정한 사진은 남고, 같은 구성의 다른 룩으로 바꾸지 않는다.
    let other = TempImage::new();
    own_image(&ctx, 0, &other.path).await;
    add_look(&ctx, 0, None, &["bottom", "top"], Some(&other.path)).await;
    sqlx::query("DELETE FROM lookbook_look WHERE id = ?")
        .bind(&look)
        .execute(&ctx.db)
        .await
        .unwrap();
    let (_, d) = ctx.post(0, "detail", json!({ "record_id": id })).await;
    assert_eq!(d["look_status"], "deleted");
    assert!(d["look"].is_null());
    assert_eq!(d["image"]["file"], file_of(&image));
    assert_eq!(d["record"]["items"], w["record"]["items"]);
}

#[tokio::test]
async fn backfill_links_only_what_is_certain() {
    let ctx = ctx!();
    let sql = include_str!("../../migrations/20261105000002_backfill_outfit_wear_photo_pin.sql");
    let photo = |image: &TempImage| image.path.clone();
    let (one, two, foreign) = (TempImage::new(), TempImage::new(), TempImage::new());
    own_image(&ctx, 0, &one.path).await;
    own_image(&ctx, 0, &two.path).await;
    own_image(&ctx, 1, &foreign.path).await;

    let record = |keys: &'static [(&'static str, &'static str)], mood: Option<&'static str>| {
        let ctx = &ctx;
        async move {
            let r = ctx.outfit_ref_with(0, 15.0, keys).await;
            sqlx::query("UPDATE outfit_ref SET context_json = JSON_SET(context_json, '$.mood_key', ?) WHERE id = ?")
                .bind(mood)
                .bind(&r)
                .execute(&ctx.db)
                .await
                .unwrap();
            let (_, p) = ctx.post(0, "plan", json!({ "outfit_ref": r })).await;
            p["record"]["record_id"].as_str().unwrap().to_string()
        }
    };

    // 정확히 하나: 연결한다.
    let exact = record(&[("top", "상의"), ("bottom", "하의")], None).await;
    let look = add_look(&ctx, 0, None, &["top", "bottom"], Some(&photo(&one))).await;
    // 무드가 같은 룩이 둘: 애매하다.
    let twins = record(&[("top-thin", "상의"), ("bottom", "하의")], None).await;
    add_look(&ctx, 0, None, &["top-thin", "bottom"], Some(&photo(&one))).await;
    add_look(&ctx, 0, None, &["bottom", "top-thin"], Some(&photo(&two))).await;
    // 무드가 다르다.
    let mood = record(&[("top-thick", "상의"), ("bottom", "하의")], Some("edgy")).await;
    add_look(
        &ctx,
        0,
        Some("glam"),
        &["top-thick", "bottom"],
        Some(&photo(&one)),
    )
    .await;
    // 사진이 다른 계정의 것이다.
    let theirs = record(&[("top", "상의"), ("shoe-heel", "신발")], None).await;
    add_look(&ctx, 0, None, &["top", "shoe-heel"], Some(&foreign.path)).await;
    // 이미 고정된 기록은 그대로다.
    let kept_look = add_look(&ctx, 0, None, &["top", "bag"], Some(&photo(&two))).await;
    let r = ctx
        .outfit_ref_with(0, 15.0, &[("top", "상의"), ("bag", "가방")])
        .await;
    let (_, p) = ctx
        .post(0, "plan", json!({ "outfit_ref": r, "look_id": kept_look }))
        .await;
    let kept = p["record"]["record_id"].as_str().unwrap().to_string();
    // 룩북 참조의 사진 없는 룩: 룩만 연결한다. 고정 전 기록을 흉내 내려고 지운다.
    let plain = add_look(&ctx, 0, None, &["outer", "bottom"], None).await;
    let (_, from) = ctx.post(0, "from-look", json!({ "look_id": plain })).await;
    let (_, p) = ctx
        .post(0, "plan", json!({ "outfit_ref": from["outfit_ref"] }))
        .await;
    let lookbook = p["record"]["record_id"].as_str().unwrap().to_string();
    sqlx::query("UPDATE outfit_wear SET pinned_look_id = NULL WHERE id = ?")
        .bind(&lookbook)
        .execute(&ctx.db)
        .await
        .unwrap();

    // 다른 테스트의 행에 영향을 주지 않게 트랜잭션 안에서 돌리고 되돌린다.
    let mut tx = ctx.db.begin().await.unwrap();
    type Pin = (
        Option<String>,
        Option<String>,
        Option<String>,
        i32,
        chrono::NaiveDateTime,
    );
    let read = |id: String| {
        format!(
            "SELECT pinned_look_id, pinned_image_path, photo_basis, revision, updated_at FROM outfit_wear WHERE id = '{id}'"
        )
    };
    let before: Pin = sqlx::query_as(&read(exact.clone()))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    for _ in 0..2 {
        sqlx::raw_sql(sql).execute(&mut *tx).await.expect("이관");
        let row: Pin = sqlx::query_as(&read(exact.clone()))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(row.0.as_deref(), Some(look.as_str()));
        assert_eq!(row.1.as_deref(), Some(one.path.as_str()));
        assert_eq!(row.2.as_deref(), Some("migrated"));
        assert_eq!(
            (row.3, row.4),
            (before.3, before.4),
            "revision·updated_at 그대로"
        );
    }
    for (name, id) in [("둘", &twins), ("무드", &mood), ("남의 사진", &theirs)] {
        let row: Pin = sqlx::query_as(&read(id.clone()))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!((row.0, row.1, row.2), (None, None, None), "{name}");
    }
    let row: Pin = sqlx::query_as(&read(kept))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(row.0.as_deref(), Some(kept_look.as_str()));
    assert_eq!(row.2.as_deref(), Some("at_save"));
    let row: Pin = sqlx::query_as(&read(lookbook))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!((row.0.as_deref(), row.1), (Some(plain.as_str()), None));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn detail_is_web_only_and_needs_login() {
    let ctx = ctx!();
    let req = Request::builder()
        .method("POST")
        .uri("/api/outfits/detail")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "record_id": "x" }).to_string()))
        .unwrap();
    assert_eq!(send(&ctx.app, req).await.0, StatusCode::UNAUTHORIZED);
    assert!(
        !crate::middleware::delegation::ALLOWED
            .iter()
            .any(|(_, p)| p.ends_with("/detail"))
    );
}

#[tokio::test]
async fn database_refuses_bad_states_directly() {
    let ctx = ctx!();
    let mine = ctx.outfit_ref(0, 15.0).await;
    let (_, p) = ctx.post(0, "plan", json!({ "outfit_ref": mine })).await;
    let id = p["record"]["record_id"].as_str().unwrap();
    for sql in [
        "UPDATE outfit_wear SET status = 'done' WHERE id = ?",
        "UPDATE outfit_wear SET reaction = 'liked' WHERE id = ?",
    ] {
        assert!(
            sqlx::query(sql).bind(id).execute(&ctx.db).await.is_err(),
            "{sql}"
        );
    }
}
