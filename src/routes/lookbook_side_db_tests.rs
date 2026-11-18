//! 룩북 옆모습을 실제 DB 로 잰다. 이미지 모델은 부르지 않는다(가짜 provider 가 호출 수를 센다).
//!
//! `TEST_DATABASE_URL` 이 없으면 건너뛴다. 이름에 `test` 가 든 DB 에서만 돈다. 시험용 PNG 는
//! `static/images/` 에 uuid 이름으로 만들고 끝나면 지운다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use crate::AppState;
use crate::middleware::auth::AuthMode;
use crate::services::llm::provider::{ImageProvider, ProviderId};
use crate::services::llm::{ImageEditRequest, ImageRequest, ImageResult, LlmError, Usage};

const OUT_PNG: &[u8] = b"\x89PNG\r\n\x1a\nside-stub";

/// (모델, 크기, 품질, 입력 바이트, 문구)
type EditCall = (String, String, String, Vec<u8>, String);

#[derive(Default)]
struct FakeImages {
    edits: AtomicU32,
    fail: AtomicBool,
    /// 마지막 편집 요청: (모델, 크기, 품질, 입력 바이트, 문구)
    last: std::sync::Mutex<Option<EditCall>>,
}

#[async_trait]
impl ImageProvider for FakeImages {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    async fn generate(&self, _model: &str, _req: &ImageRequest) -> Result<ImageResult, LlmError> {
        panic!("옆모습은 새로 생성하지 않고 편집만 한다");
    }

    async fn edit(&self, model: &str, req: &ImageEditRequest) -> Result<ImageResult, LlmError> {
        self.edits.fetch_add(1, Ordering::SeqCst);
        *self.last.lock().unwrap() = Some((
            model.to_string(),
            req.size.clone(),
            req.quality.clone(),
            req.base_png.clone(),
            req.prompt.clone(),
        ));
        // 동시 요청이 겹칠 시간을 준다.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if self.fail.load(Ordering::SeqCst) {
            // 재시도하지 않는 오류로 낸다 — 한 번 누름에 한 번 호출임을 센다.
            return Err(LlmError::Config("가짜 실패".into()));
        }
        use base64::Engine;
        Ok(ImageResult {
            b64_png: base64::engine::general_purpose::STANDARD.encode(OUT_PNG),
            usage: Usage {
                input_tokens: 1883,
                output_tokens: 664,
            },
        })
    }
}

struct Ctx {
    db: sqlx::MySqlPool,
    app: Router,
    fake: Arc<FakeImages>,
    users: [(String, String); 2],
    files: std::sync::Mutex<Vec<String>>,
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
    let users = [0, 1].map(|i| (format!("side-{run}-{i}"), format!("tok-side-{run}-{i}")));
    for (id, token) in &users {
        sqlx::query("INSERT INTO app_user (id, username, api_token) VALUES (?, ?, ?)")
            .bind(id)
            .bind(id)
            .bind(token)
            .execute(&db)
            .await
            .expect("사용자");
    }
    let fake = Arc::new(FakeImages::default());
    let http = reqwest::Client::new();
    let llm = Arc::new(
        crate::services::llm::LlmClient::from_env(http.clone()).with_image_provider(fake.clone()),
    );
    let embedding =
        Arc::new(crate::services::embedding::EmbeddingService::new(llm.clone()).expect("임베딩"));
    let state = AppState {
        db: db.clone(),
        http_client: http,
        llm,
        kma_api_key: String::new(),
        embedding,
        auth_mode: AuthMode::Strict,
        admin_users: Arc::new(Vec::new()),
        delegation_key: None,
    };
    let app = Router::new()
        .nest("/api", crate::routes::api_router())
        .with_state(state);
    Some(Ctx {
        db,
        app,
        fake,
        users,
        files: Default::default(),
    })
}

impl Ctx {
    fn user(&self, u: usize) -> &str {
        &self.users[u].0
    }

    /// 원본 사진 파일과 캐시 행, 그 사진을 붙인 룩을 만든다. 룩 id 와 원본 경로를 돌려준다.
    async fn look(&self, u: usize, presentation: &str, with_file: bool) -> (String, String) {
        let file = format!("{}.png", uuid::Uuid::new_v4());
        let path = format!("/static/images/{file}");
        if with_file {
            let bytes = [b"\x89PNG\r\n\x1a\n".as_slice(), file.as_bytes()].concat();
            std::fs::write(format!("static/images/{file}"), bytes).unwrap();
            self.files.lock().unwrap().push(path.clone());
        }
        let status = if presentation == "model" {
            "passed"
        } else {
            "not_applicable"
        };
        sqlx::query(
            "INSERT INTO outfit_image (id, user_id, outfit_hash, prompt_hash, image_path, presentation, verification_status) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(self.user(u))
        .bind(&file[..16])
        .bind(&file[..16])
        .bind(&path)
        .bind(presentation)
        .bind(status)
        .execute(&self.db)
        .await
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO lookbook_look (id, user_id, mood_key, title, image_prompt, image_path, outfit_json) \
             VALUES (?, ?, 'edgy', 't', ?, ?, '[]')",
        )
        .bind(&id)
        .bind(self.user(u))
        .bind(&id)
        .bind(&path)
        .execute(&self.db)
        .await
        .unwrap();
        (id, path)
    }

    async fn call(&self, u: usize, method: &str, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {}", self.users[u].1))
            .body(Body::empty())
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 22)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn side(&self, u: usize, method: &str, id: &str) -> (StatusCode, Value) {
        self.call(u, method, &format!("/api/lookbook/{id}/side"))
            .await
    }

    fn edits(&self) -> u32 {
        self.fake.edits.load(Ordering::SeqCst)
    }

    async fn cleanup(&self) {
        let mut files = self.files.lock().unwrap().clone();
        for u in 0..2 {
            let side: Vec<(Option<String>,)> =
                sqlx::query_as("SELECT image_path FROM lookbook_side_image WHERE user_id = ?")
                    .bind(self.user(u))
                    .fetch_all(&self.db)
                    .await
                    .unwrap();
            files.extend(side.into_iter().filter_map(|r| r.0));
            for table in [
                "lookbook_side_image",
                "lookbook_look",
                "outfit_image",
                "app_user",
            ] {
                let col = if table == "app_user" { "id" } else { "user_id" };
                sqlx::query(&format!("DELETE FROM {table} WHERE {col} = ?"))
                    .bind(self.user(u))
                    .execute(&self.db)
                    .await
                    .unwrap();
            }
        }
        for f in files {
            let _ = std::fs::remove_file(f.trim_start_matches('/'));
        }
    }
}

#[tokio::test]
async fn side_view_is_made_once_per_account_and_photo() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let (look, source) = ctx.look(0, "model", true).await;
    let source_bytes = std::fs::read(source.trim_start_matches('/')).unwrap();

    // 처음엔 없다. GET 은 만들지 않는다.
    let (st, body) = ctx.side(0, "GET", &look).await;
    assert_eq!(
        (st, body["status"].as_str()),
        (StatusCode::OK, Some("none"))
    );
    assert_eq!(ctx.edits(), 0);

    // 동시에 다섯 번 눌러도 편집은 한 번.
    let (a, b, c, d, e) = tokio::join!(
        ctx.side(0, "POST", &look),
        ctx.side(0, "POST", &look),
        ctx.side(0, "POST", &look),
        ctx.side(0, "POST", &look),
        ctx.side(0, "POST", &look),
    );
    let results = [a, b, c, d, e];
    assert_eq!(ctx.edits(), 1, "동시 클릭이 중복 호출됐다");
    let ready: Vec<_> = results
        .iter()
        .filter(|(_, b)| b["status"] == "ready")
        .collect();
    assert_eq!(ready.len(), 1);
    assert!(results.iter().all(
        |(st, b)| *st == StatusCode::OK && (b["status"] == "ready" || b["status"] == "pending")
    ));

    // 요청 내용: 2.6 sunburst, low, 1024x1536, 이 룩의 원본 바이트 그대로.
    let (model, size, quality, input, prompt) = ctx.fake.last.lock().unwrap().clone().unwrap();
    assert_eq!(model, "gpt-image-2.6-sunburst");
    assert_eq!((size.as_str(), quality.as_str()), ("1024x1536", "low"));
    assert_eq!(input, source_bytes);
    assert_eq!(prompt, super::SIDE_PROMPT);

    let side_path = ready[0].1["image_path"].as_str().unwrap().to_string();
    assert_ne!(side_path, source);
    assert_eq!(
        std::fs::read(side_path.trim_start_matches('/')).unwrap(),
        OUT_PNG
    );
    // 원본은 그대로.
    assert_eq!(
        std::fs::read(source.trim_start_matches('/')).unwrap(),
        source_bytes
    );

    // 다시 눌러도(새로고침·재시작과 같다 — 상태는 DB 에만 있다) 저장된 것을 준다.
    let (_, again) = ctx.side(0, "POST", &look).await;
    assert_eq!(again["image_path"].as_str(), Some(side_path.as_str()));
    assert_eq!(ctx.edits(), 1);

    // 목록에 원본과 옆모습이 함께 실린다.
    let (_, list) = ctx.call(0, "GET", "/api/lookbook").await;
    let row = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == look.as_str())
        .unwrap();
    assert_eq!(row["image_path"].as_str(), Some(source.as_str()));
    assert_eq!(row["side_image_path"].as_str(), Some(side_path.as_str()));
    assert_eq!(row["side_status"], "ready");
    assert_eq!(row["side_eligible"], true);

    // 옆모습 사진은 내 계정에만 내보낸다.
    let file = side_path.trim_start_matches("/static/images/");
    let (st, _) = ctx.call(0, "GET", &format!("/api/images/{file}")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = ctx.call(1, "GET", &format!("/api/images/{file}")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // 남의 룩은 보이지도, 만들어지지도 않는다.
    for method in ["GET", "POST"] {
        let (st, _) = ctx.side(1, method, &look).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }
    assert_eq!(ctx.edits(), 1);

    ctx.cleanup().await;
}

#[tokio::test]
async fn flat_lay_and_missing_photos_are_not_targets() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let (flat, _) = ctx.look(0, "flat_lay", true).await;
    let (missing, _) = ctx.look(0, "model", false).await;
    // 사진이 아예 없는 룩.
    let bare = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO lookbook_look (id, user_id, title, image_prompt, outfit_json) VALUES (?, ?, 't', ?, '[]')",
    )
    .bind(&bare)
    .bind(ctx.user(0))
    .bind(&bare)
    .execute(&ctx.db)
    .await
    .unwrap();

    for id in [&flat, &missing, &bare] {
        let (st, _) = ctx.side(0, "POST", id).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{id}");
    }
    let (st, _) = ctx.side(0, "POST", "../../etc").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(ctx.edits(), 0);

    let (_, list) = ctx.call(0, "GET", "/api/lookbook").await;
    for r in list.as_array().unwrap() {
        assert_eq!(r["side_eligible"], false, "{}", r["id"]);
    }
    ctx.cleanup().await;
}

#[tokio::test]
async fn failures_can_be_retried_up_to_the_limit_and_stale_pending_is_reclaimed() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let (look, source) = ctx.look(0, "model", true).await;

    ctx.fake.fail.store(true, Ordering::SeqCst);
    let (st, b) = ctx.side(0, "POST", &look).await;
    assert_eq!(
        (st, b["status"].as_str(), b["retryable"].as_bool()),
        (StatusCode::OK, Some("failed"), Some(true))
    );
    assert_eq!(ctx.edits(), 1);
    // 실패 상태를 다시 보는 것만으로는 호출하지 않는다.
    let (_, b) = ctx.side(0, "GET", &look).await;
    assert_eq!(b["status"], "failed");
    assert_eq!(ctx.edits(), 1);

    // 다시 시도하면 된다.
    ctx.fake.fail.store(false, Ordering::SeqCst);
    let (_, b) = ctx.side(0, "POST", &look).await;
    assert_eq!(b["status"], "ready");
    assert_eq!(ctx.edits(), 2);

    // 서버가 생성 도중 꺼져 남은 pending: 오래되기 전엔 기다리고, 오래되면 다시 잡는다.
    let (look2, source2) = ctx.look(0, "model", true).await;
    sqlx::query(
        "INSERT INTO lookbook_side_image (user_id, source_path, status, attempts, claim_token, claimed_at) \
         VALUES (?, ?, 'pending', 1, 'dead', NOW())",
    )
    .bind(ctx.user(0))
    .bind(&source2)
    .execute(&ctx.db)
    .await
    .unwrap();
    let (_, b) = ctx.side(0, "POST", &look2).await;
    assert_eq!(b["status"], "pending");
    assert_eq!(ctx.edits(), 2);
    sqlx::query("UPDATE lookbook_side_image SET claimed_at = NOW() - INTERVAL 11 MINUTE WHERE user_id = ? AND source_path = ?")
        .bind(ctx.user(0))
        .bind(&source2)
        .execute(&ctx.db)
        .await
        .unwrap();
    let (_, b) = ctx.side(0, "POST", &look2).await;
    assert_eq!(b["status"], "ready");
    assert_eq!(ctx.edits(), 3);

    // 상한까지 실패하면 더 부르지 않는다.
    let (look3, _) = ctx.look(0, "model", true).await;
    ctx.fake.fail.store(true, Ordering::SeqCst);
    for _ in 0..super::MAX_ATTEMPTS {
        ctx.side(0, "POST", &look3).await;
    }
    let before = ctx.edits();
    let (_, b) = ctx.side(0, "POST", &look3).await;
    assert_eq!(
        (b["status"].as_str(), b["retryable"].as_bool()),
        (Some("failed"), Some(false))
    );
    assert_eq!(ctx.edits(), before);
    assert_eq!(before, 3 + super::MAX_ATTEMPTS as u32);

    let _ = source;
    ctx.cleanup().await;
}
