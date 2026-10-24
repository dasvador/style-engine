//! 인증(fail closed)과 계정별 옷장·사진 격리를 실제 DB 로 잰다.
//!
//! `TEST_DATABASE_URL` 이 없으면 건너뛴다(통과가 아니다 — 출력에 "건너뜁니다"가 찍힌다). 그 DB 에
//! 마이그레이션을 돌리므로 이름에 `test` 가 든 DB 에서만 돈다. 모델·이미지 API 는 부르지 않는다.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::AppState;
use crate::middleware::auth::AuthMode;

struct Ctx {
    db: sqlx::MySqlPool,
    url: String,
    run: String,
    /// (user_id, token)
    users: [(String, String); 2],
}

async fn setup() -> Option<Ctx> {
    let url = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let db_name = db_name(&url);
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
    let users = [0, 1].map(|i| {
        (
            format!("auth-test-{run}-{i}"),
            format!("tok-auth-{run}-{i}"),
        )
    });
    for (id, token) in &users {
        sqlx::query("INSERT INTO app_user (id, username, api_token) VALUES (?, ?, ?)")
            .bind(id)
            .bind(id)
            .bind(token)
            .execute(&db)
            .await
            .expect("사용자");
    }
    Some(Ctx {
        db,
        url,
        run,
        users,
    })
}

fn db_name(url: &str) -> &str {
    url.rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
}

fn app(ctx: &Ctx, mode: AuthMode) -> Router {
    app_with_admins(ctx, mode, Vec::new())
}

fn app_with_admins(ctx: &Ctx, mode: AuthMode, admins: Vec<String>) -> Router {
    let http = reqwest::Client::new();
    let llm = std::sync::Arc::new(crate::services::llm::LlmClient::from_env(http.clone()));
    let embedding = std::sync::Arc::new(
        crate::services::embedding::EmbeddingService::new(llm.clone()).expect("임베딩"),
    );
    let state = AppState {
        db: ctx.db.clone(),
        http_client: http,
        llm,
        kma_api_key: String::new(),
        embedding,
        auth_mode: mode,
        admin_users: std::sync::Arc::new(admins),
    };
    // main.rs 와 같은 /static 보호를 건다.
    Router::new()
        .nest("/api", crate::routes::api_router())
        .nest_service(
            "/static",
            Router::new()
                .fallback_service(tower_http::services::ServeDir::new("static"))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    crate::middleware::static_guard::local_dev_only,
                ))
                .with_state(()),
        )
        .with_state(state)
}

#[derive(Default)]
struct Opts<'a> {
    token: Option<&'a str>,
    raw_auth: Option<&'a str>,
    body: Option<serde_json::Value>,
    /// 연결 상대 주소(서버가 connect_info 로 붙이는 값)
    peer: Option<&'a str>,
    headers: Vec<(&'a str, &'a str)>,
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    o: Opts<'_>,
) -> (StatusCode, serde_json::Value, axum::http::HeaderMap) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(t) = o.token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    if let Some(raw) = o.raw_auth {
        req = req.header("authorization", raw);
    }
    for (k, v) in &o.headers {
        req = req.header(*k, *v);
    }
    let mut req = match o.body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    if let Some(peer) = o.peer {
        let addr: SocketAddr = peer.parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 22)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json, headers)
}

fn tok(ctx: &Ctx, i: usize) -> Option<&str> {
    Some(ctx.users[i].1.as_str())
}

#[tokio::test]
async fn missing_invalid_and_malformed_tokens_are_rejected() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let app = app(&ctx, AuthMode::Strict);
    for path in [
        "/api/clothes",
        "/api/user/me",
        "/api/lookbook",
        "/api/chat/history",
        "/api/region",
    ] {
        let (st, _, h) = call(&app, "GET", path, Opts::default()).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{path} 토큰 없음");
        assert!(
            h.get("www-authenticate")
                .is_some_and(|v| v.to_str().unwrap().starts_with("Bearer ")),
            "{path} WWW-Authenticate"
        );
    }
    for raw in [
        "Bearer nope-not-a-token",
        "Bearer ",
        "Basic dXNlcjpwYXNz",
        "bearer tok-x",
        "Bearer dev-token-default",
    ] {
        let (st, _, _) = call(
            &app,
            "GET",
            "/api/clothes",
            Opts {
                raw_auth: Some(raw),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{raw}");
    }
    let long = format!("Bearer {}", "a".repeat(500));
    let (st, _, _) = call(
        &app,
        "GET",
        "/api/clothes",
        Opts {
            raw_auth: Some(&long),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // 알려진 개발 토큰은 마이그레이션이 무효화했다.
    let default_token: String =
        sqlx::query_scalar("SELECT api_token FROM app_user WHERE id = 'default'")
            .fetch_one(&ctx.db)
            .await
            .unwrap();
    assert_ne!(default_token, "dev-token-default");
    let revoked = format!("Bearer {default_token}");
    let (st, _, _) = call(
        &app,
        "GET",
        "/api/clothes",
        Opts {
            raw_auth: Some(&revoked),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "무효화된 default 토큰");

    // 맞는 토큰 → 나
    let (st, me, _) = call(
        &app,
        "GET",
        "/api/user/me",
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["user_id"], ctx.users[0].0);
    assert!(me.get("api_token").is_none(), "토큰을 돌려주지 않는다");
}

#[tokio::test]
async fn wardrobe_is_isolated_between_accounts() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let app = app(&ctx, AuthMode::Strict);
    let a = || Opts {
        token: tok(&ctx, 0),
        ..Default::default()
    };
    let b = || Opts {
        token: tok(&ctx, 1),
        ..Default::default()
    };

    let (st, created, _) = call(
        &app,
        "POST",
        "/api/clothes",
        Opts {
            token: tok(&ctx, 0),
            body: Some(serde_json::json!({"name": format!("A 니트 {}", ctx.run), "category": "상의", "color": "네이비", "seasons": ["가을"]})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    let owner: String = sqlx::query_scalar("SELECT user_id FROM clothing WHERE id = ?")
        .bind(&id)
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(owner, ctx.users[0].0, "본문이 아니라 토큰 주인이 소유자");

    // A 는 보인다
    let (st, list_a, _) = call(&app, "GET", "/api/clothes", a()).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        list_a
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == id.as_str())
    );
    let (st, _, _) = call(&app, "GET", &format!("/api/clothes/{id}"), a()).await;
    assert_eq!(st, StatusCode::OK);

    // B 에게는 없다 — 목록·조회·수정·삭제·장르·코디 평가 모두
    let (st, list_b, _) = call(&app, "GET", "/api/clothes", b()).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !list_b
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == id.as_str())
    );
    assert!(
        !list_b
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["user_id"].is_string()),
        "응답에 소유자 id 를 싣지 않는다"
    );
    let (st, _, _) = call(&app, "GET", &format!("/api/clothes/{id}"), b()).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = call(
        &app,
        "PUT",
        &format!("/api/clothes/{id}"),
        Opts {
            token: tok(&ctx, 1),
            body: Some(serde_json::json!({"name": "B 가 바꿈"})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = call(
        &app,
        "PUT",
        &format!("/api/clothes/{id}/genres"),
        Opts {
            token: tok(&ctx, 1),
            body: Some(serde_json::json!({"style_genres": ["minimal"]})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = call(&app, "DELETE", &format!("/api/clothes/{id}"), b()).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = call(
        &app,
        "POST",
        "/api/outfit/evaluate",
        Opts {
            token: tok(&ctx, 1),
            body: Some(serde_json::json!({"top": id, "bottom": id})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "남의 아이템으로 코디 평가");

    // 아무것도 바뀌지 않았다
    let (name, genres): (String, i64) = sqlx::query_as(
        "SELECT c.name, (SELECT COUNT(*) FROM clothing_style_genre g WHERE g.clothing_id = c.id) FROM clothing c WHERE c.id = ?",
    )
    .bind(&id)
    .fetch_one(&ctx.db)
    .await
    .unwrap();
    assert!(name.starts_with("A 니트"));
    assert_eq!(genres, 0);

    // 토큰 없이는 아무것도 못 한다
    let (st, _, _) = call(
        &app,
        "DELETE",
        &format!("/api/clothes/{id}"),
        Opts::default(),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // A 는 고치고 지울 수 있다
    let (st, updated, _) = call(
        &app,
        "PUT",
        &format!("/api/clothes/{id}"),
        Opts {
            token: tok(&ctx, 0),
            body: Some(serde_json::json!({"color": "차콜"})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{updated}");
    assert_eq!(updated["color"], "차콜");
    let (st, _, _) = call(&app, "DELETE", &format!("/api/clothes/{id}"), a()).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = call(&app, "GET", &format!("/api/clothes/{id}"), a()).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn existing_default_wardrobe_stays_default_and_is_not_visible_to_new_accounts() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    // 소유자 없이 들어온 옛 행(마이그레이션 전 데이터 흉내)은 default 소유가 된다.
    let legacy = format!("legacy-{}", ctx.run);
    sqlx::query("INSERT INTO clothing (id, name, category) VALUES (?, '옛 셔츠', '상의')")
        .bind(&legacy)
        .execute(&ctx.db)
        .await
        .unwrap();
    let owner: String = sqlx::query_scalar("SELECT user_id FROM clothing WHERE id = ?")
        .bind(&legacy)
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(owner, "default");

    let app = app(&ctx, AuthMode::Strict);
    let (st, list, _) = call(
        &app,
        "GET",
        "/api/clothes",
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !list
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == legacy.as_str())
    );

    // 개발 모드의 로컬 요청만 default 로 읽는다.
    let dev = self::app(&ctx, AuthMode::DevDefaultLoopback);
    let local = || Opts {
        peer: Some("127.0.0.1:50000"),
        headers: vec![("host", "localhost:3003")],
        ..Default::default()
    };
    let (st, list, _) = call(&dev, "GET", "/api/clothes", local()).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == legacy.as_str())
    );

    // 개발 모드여도: 연결 정보 없음 / 원격 주소 / 터널 Host / 전달 헤더 / 다른 사이트 Origin → 401
    let cases: Vec<Opts> = vec![
        Opts {
            headers: vec![("host", "localhost:3003")],
            ..Default::default()
        },
        Opts {
            peer: Some("203.0.113.9:4000"),
            headers: vec![("host", "localhost:3003")],
            ..Default::default()
        },
        Opts {
            peer: Some("127.0.0.1:50000"),
            headers: vec![("host", "abc.trycloudflare.com")],
            ..Default::default()
        },
        Opts {
            peer: Some("127.0.0.1:50000"),
            headers: vec![
                ("host", "localhost:3003"),
                ("x-forwarded-for", "203.0.113.9"),
            ],
            ..Default::default()
        },
        Opts {
            peer: Some("127.0.0.1:50000"),
            headers: vec![
                ("host", "localhost:3003"),
                ("origin", "https://evil.example"),
            ],
            ..Default::default()
        },
    ];
    for (i, o) in cases.into_iter().enumerate() {
        let (st, _, _) = call(&dev, "GET", "/api/clothes", o).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "case {i}");
    }
    // 개발 모드에서도 틀린 토큰은 default 로 바뀌지 않는다.
    let (st, _, _) = call(
        &dev,
        "GET",
        "/api/clothes",
        Opts {
            raw_auth: Some("Bearer wrong"),
            ..local()
        },
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    sqlx::query("DELETE FROM clothing WHERE id = ?")
        .bind(&legacy)
        .execute(&ctx.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn generated_images_only_for_owner_and_static_closed() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let file = format!("{}.png", uuid::Uuid::new_v4());
    let path = std::path::Path::new("static/images").join(&file);
    std::fs::create_dir_all("static/images").unwrap();
    std::fs::write(
        &path,
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0],
    )
    .unwrap();
    sqlx::query(
        "INSERT INTO outfit_image (id, user_id, outfit_hash, prompt_hash, image_path) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&ctx.users[0].0)
    .bind(format!("auth-test-{}", ctx.run))
    .bind(format!("auth-test-{}", ctx.run))
    .bind(format!("/static/images/{file}"))
    .execute(&ctx.db)
    .await
    .unwrap();

    let app = app(&ctx, AuthMode::Strict);
    let url = format!("/api/images/{file}");
    let res = app
        .clone()
        .oneshot(
            Request::get(&url)
                .header("authorization", format!("Bearer {}", ctx.users[0].1))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/png");

    let (st, _, _) = call(
        &app,
        "GET",
        &url,
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "남의 사진");
    let (st, _, _) = call(&app, "GET", &url, Opts::default()).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    for bad in [
        "/api/images/..%2f..%2f.env",
        "/api/images/x.png",
        "/api/images/%2e%2e",
    ] {
        let (st, _, _) = call(
            &app,
            "GET",
            bad,
            Opts {
                token: tok(&ctx, 0),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{bad}");
    }

    // 공개 /static 은 엄격 모드에서 닫혀 있다(토큰이 있어도).
    let (st, _, _) = call(
        &app,
        "GET",
        &format!("/static/images/{file}"),
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // 개발 모드의 로컬 요청만 연다.
    let dev = self::app(&ctx, AuthMode::DevDefaultLoopback);
    let (st, _, _) = call(
        &dev,
        "GET",
        &format!("/static/images/{file}"),
        Opts {
            peer: Some("127.0.0.1:1"),
            headers: vec![("host", "127.0.0.1:3003")],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = call(
        &dev,
        "GET",
        &format!("/static/images/{file}"),
        Opts {
            peer: Some("127.0.0.1:1"),
            headers: vec![("host", "abc.trycloudflare.com")],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "터널 경유");

    std::fs::remove_file(&path).unwrap();
    sqlx::query("DELETE FROM outfit_image WHERE outfit_hash = ?")
        .bind(format!("auth-test-{}", ctx.run))
        .execute(&ctx.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn register_does_not_hand_out_existing_accounts() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let app = app(&ctx, AuthMode::Strict);
    for username in [ctx.users[0].0.as_str(), "default"] {
        let (st, body, _) = call(
            &app,
            "POST",
            "/api/user/register",
            Opts {
                body: Some(serde_json::json!({"username": username})),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "{username}");
        assert!(body.get("api_token").is_none());
    }
}

/// 새 마이그레이션이 기존 행을 지우거나 다른 계정에 넘기지 않는지, 마이그레이션 직전 상태의 별도 DB 에서 잰다.
#[tokio::test]
async fn owner_migration_is_non_destructive() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let base = ctx.url.rsplit_once('/').unwrap().0.to_string();
    let scratch = format!("{}_mig_{}", db_name(&ctx.url), ctx.run);
    sqlx::query(&format!("CREATE DATABASE `{scratch}`"))
        .execute(&ctx.db)
        .await
        .expect("임시 DB 만들기");
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&format!("{base}/{scratch}"))
        .await
        .unwrap();

    let full = sqlx::migrate!("./migrations");
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            full.migrations
                .iter()
                .filter(|m| m.version < 20261024000001)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    };
    before.run(&pool).await.expect("이전 마이그레이션");
    for (id, name) in [("old-1", "옛 셔츠"), ("old-2", "옛 바지")] {
        sqlx::query("INSERT INTO clothing (id, name, category) VALUES (?, ?, '상의')")
            .bind(id)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO outfit_image (id, outfit_hash, prompt_hash, image_path) VALUES ('img-old', 'oh', 'ph', '/static/images/old.png')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let regions_before: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM region_setting ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!regions_before.is_empty());
    let token_before: String =
        sqlx::query_scalar("SELECT api_token FROM app_user WHERE id = 'default'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(token_before, "dev-token-default");

    // 시드 마이그레이션이 넣은 데모 옷도 같이 있다. 전후로 행·이름이 같아야 한다.
    let before_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM clothing ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    full.run(&pool).await.expect("새 마이그레이션");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, name, user_id FROM clothing ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows.iter()
            .map(|(i, n, _)| (i.clone(), n.clone()))
            .collect::<Vec<_>>(),
        before_rows,
        "행을 지우거나 바꾸지 않는다"
    );
    assert!(
        rows.iter().all(|(_, _, owner)| owner == "default"),
        "모두 default 소유로 남는다"
    );
    assert!(rows.iter().any(|(i, n, _)| i == "old-1" && n == "옛 셔츠"));
    let (users, token_after): (i64, String) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM app_user), (SELECT api_token FROM app_user WHERE id = 'default')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(users, 1, "default 행은 남는다");
    assert!(token_after.starts_with("revoked-"), "알려진 토큰만 무효화");

    // 사진 캐시: 행은 그대로, 유니크 키는 (user_id, outfit_hash, prompt_hash)
    let img: (String, String, String) =
        sqlx::query_as("SELECT id, user_id, image_path FROM outfit_image WHERE id = 'img-old'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        img,
        (
            "img-old".into(),
            "default".into(),
            "/static/images/old.png".into()
        )
    );
    let key_cols: Vec<(String,)> = sqlx::query_as(
        "SELECT COLUMN_NAME FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() \
         AND TABLE_NAME = 'outfit_image' AND INDEX_NAME = 'idx_user_outfit_prompt' ORDER BY SEQ_IN_INDEX",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        key_cols.into_iter().map(|c| c.0).collect::<Vec<_>>(),
        vec!["user_id", "outfit_hash", "prompt_hash"]
    );
    // 지역: 행은 그대로, 모두 default 소유
    let regions_after: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, name, user_id FROM region_setting ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        regions_after
            .iter()
            .map(|(i, n, _)| (i.clone(), n.clone()))
            .collect::<Vec<_>>(),
        regions_before
    );
    assert!(regions_after.iter().all(|(_, _, u)| u == "default"));

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE `{scratch}`"))
        .execute(&ctx.db)
        .await
        .expect("임시 DB 지우기");
}

/// 같은 착장 문구(이미지 프롬프트)를 두 계정이 쓸 때 사진이 섞이지 않는다.
///   - B 의 룩북 저장이 A 의 사진 캐시를 붙이지 않는다(cached_image_path).
///   - B 가 사진을 붙여도 A 의 룩에 쓰지 않는다(pin_image 의 타계정 쓰기).
///   - B 의 사진 요청이 A 의 룩에 핀된 사진·A 의 캐시를 받지 않고, A 행을 갱신하지 않는다.
///   - 남의 사진 경로가 룩에 붙어 있어도 그것이 사진 접근권이 되지 않는다.
/// 생성(유료)은 일어나지 않게, B 의 요청은 B 자신의 캐시에 적중하도록 준비한다.
#[tokio::test]
async fn same_prompt_never_crosses_accounts() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let app = app(&ctx, AuthMode::Strict);
    let (ua, ub) = (&ctx.users[0].0, &ctx.users[1].0);
    let prompt = format!(
        "top/inner: 같은 프롬프트 테스트 셔츠 {}, pants: 블랙 슬랙스",
        ctx.run
    );
    let genre = crate::models::style_vocab::StyleGenre::Minimal;
    let (oh, ph, _, _) = crate::routes::chat::image_cache_keys(&prompt, genre);
    let file_a = format!("{}.png", uuid::Uuid::new_v4());
    let file_b = format!("{}.png", uuid::Uuid::new_v4());
    std::fs::create_dir_all("static/images").unwrap();
    for f in [&file_a, &file_b] {
        std::fs::write(
            std::path::Path::new("static/images").join(f),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0],
        )
        .unwrap();
    }
    // A 만 이 문구의 사진을 가지고 있다.
    sqlx::query(
        "INSERT INTO outfit_image (id, user_id, outfit_hash, prompt_hash, image_path, mood, verification_status, hit_count) \
         VALUES (?, ?, ?, ?, ?, 'minimal', 'passed', 0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(ua)
    .bind(&oh)
    .bind(&ph)
    .bind(format!("/static/images/{file_a}"))
    .execute(&ctx.db)
    .await
    .unwrap();

    fn look<'a>(token: &'a str, prompt: &str) -> Opts<'a> {
        Opts {
            token: Some(token),
            body: Some(serde_json::json!({
                "mood_key": "minimal", "title": "같은 코디", "image_prompt": prompt, "outfit_json": {}
            })),
            ..Default::default()
        }
    }
    // B 가 같은 문구로 룩을 담는다 → A 의 사진이 붙으면 안 된다.
    let (st, saved_b, _) = call(
        &app,
        "POST",
        "/api/lookbook",
        look(&ctx.users[1].1, &prompt),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{saved_b}");
    let b_path: Option<String> = sqlx::query_scalar(
        "SELECT image_path FROM lookbook_look WHERE user_id = ? AND image_prompt = ?",
    )
    .bind(ub)
    .bind(&prompt)
    .fetch_one(&ctx.db)
    .await
    .unwrap();
    assert_eq!(b_path, None, "B 의 룩에 A 의 캐시 사진이 붙었다");
    let (_, list_b, _) = call(
        &app,
        "GET",
        "/api/lookbook",
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert!(
        !list_b.to_string().contains(&file_a),
        "목록에서도 A 사진이 붙지 않는다"
    );

    // A 도 같은 문구의 룩을 담는다 → A 는 자기 사진이 붙는다.
    let (st, _, _) = call(
        &app,
        "POST",
        "/api/lookbook",
        look(&ctx.users[0].1, &prompt),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let a_path: Option<String> = sqlx::query_scalar(
        "SELECT image_path FROM lookbook_look WHERE user_id = ? AND image_prompt = ?",
    )
    .bind(ua)
    .bind(&prompt)
    .fetch_one(&ctx.db)
    .await
    .unwrap();
    assert_eq!(
        a_path.as_deref(),
        Some(format!("/static/images/{file_a}").as_str())
    );

    // B 가 자기 사진을 만들어(캐시 행 생성) 같은 문구로 요청한다 → A 의 핀·캐시가 아니라 자기 사진.
    sqlx::query(
        "INSERT INTO outfit_image (id, user_id, outfit_hash, prompt_hash, image_path, mood, verification_status, hit_count) \
         VALUES (?, ?, ?, ?, ?, 'minimal', 'passed', 0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(ub)
    .bind(&oh)
    .bind(&ph)
    .bind(format!("/static/images/{file_b}"))
    .execute(&ctx.db)
    .await
    .expect("같은 해시라도 계정이 다르면 각자 행을 가진다(유니크 키에 user_id)");
    let (st, img, _) = call(
        &app,
        "POST",
        "/api/chat/image",
        Opts {
            token: tok(&ctx, 1),
            body: Some(serde_json::json!({"items": prompt, "mood": "minimal"})),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{img}");
    assert_eq!(
        img["image_url"],
        format!("/static/images/{file_b}"),
        "A 의 핀·캐시 사진을 받으면 안 된다"
    );
    let hits: Vec<(String, i32)> = sqlx::query_as(
        "SELECT user_id, hit_count FROM outfit_image WHERE outfit_hash = ? AND prompt_hash = ? ORDER BY user_id",
    )
    .bind(&oh)
    .bind(&ph)
    .fetch_all(&ctx.db)
    .await
    .unwrap();
    let hit = |u: &str| hits.iter().find(|(id, _)| id == u).map(|(_, n)| *n);
    assert_eq!(hit(ua), Some(0), "A 의 캐시 행은 갱신되지 않는다");
    assert_eq!(hit(ub), Some(1));

    // B 가 다시 룩북을 열면 B 의 룩에 B 사진만 붙고, A 의 룩은 그대로다.
    let (_, _, _) = call(
        &app,
        "GET",
        "/api/lookbook",
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    let paths: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT user_id, image_path FROM lookbook_look WHERE image_prompt = ? ORDER BY user_id",
    )
    .bind(&prompt)
    .fetch_all(&ctx.db)
    .await
    .unwrap();
    for (u, p) in &paths {
        let expected = if u == ua { &file_a } else { &file_b };
        assert_eq!(
            p.as_deref(),
            Some(format!("/static/images/{expected}").as_str()),
            "{u}"
        );
    }

    // 남의 사진 경로가 내 룩에 붙어 있어도(예전 데이터) 그것이 접근권이 되지 않는다.
    sqlx::query("UPDATE lookbook_look SET image_path = ? WHERE user_id = ? AND image_prompt = ?")
        .bind(format!("/static/images/{file_a}"))
        .bind(ub)
        .bind(&prompt)
        .execute(&ctx.db)
        .await
        .unwrap();
    let (st, _, _) = call(
        &app,
        "GET",
        &format!("/api/images/{file_a}"),
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "룩북 경로는 사진 접근권이 아니다"
    );
    let (st, _, h) = call(
        &app,
        "GET",
        &format!("/api/images/{file_a}"),
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(h["cache-control"], "private, no-store");
    assert_eq!(h["vary"], "Authorization");

    for f in [&file_a, &file_b] {
        let _ = std::fs::remove_file(std::path::Path::new("static/images").join(f));
    }
    sqlx::query("DELETE FROM outfit_image WHERE outfit_hash = ?")
        .bind(&oh)
        .execute(&ctx.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM lookbook_look WHERE image_prompt = ?")
        .bind(&prompt)
        .execute(&ctx.db)
        .await
        .unwrap();
}

/// 지역(위치)은 계정별: B 는 A 의 지역을 읽거나 지우거나 바꾸지 못한다.
#[tokio::test]
async fn region_is_per_account() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let app = app(&ctx, AuthMode::Strict);
    let put = |i: usize, name: &str| Opts {
        token: tok(&ctx, i),
        body: Some(serde_json::json!({"name": name, "latitude": 35.1, "longitude": 129.0})),
        ..Default::default()
    };
    let (st, _, _) = call(
        &app,
        "GET",
        "/api/region",
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "새 계정은 지역이 없다(다른 계정 위치를 대신 쓰지 않는다)"
    );
    let (st, _, _) = call(&app, "PUT", "/api/region", put(0, "부산")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = call(
        &app,
        "GET",
        "/api/region",
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "B 는 A 의 지역을 못 본다");
    let (st, _, _) = call(&app, "PUT", "/api/region", put(1, "대전")).await;
    assert_eq!(st, StatusCode::OK);
    let (_, a, _) = call(
        &app,
        "GET",
        "/api/region",
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        a["name"], "부산",
        "B 의 저장이 A 의 지역을 지우거나 바꾸지 않는다"
    );
    let (_, b, _) = call(
        &app,
        "GET",
        "/api/region",
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(b["name"], "대전");
    let default_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM region_setting WHERE user_id = 'default'")
            .fetch_one(&ctx.db)
            .await
            .unwrap();
    assert!(default_rows >= 1, "기존(default) 지역은 남아 있다");
    sqlx::query("DELETE FROM region_setting WHERE user_id IN (?, ?)")
        .bind(&ctx.users[0].0)
        .bind(&ctx.users[1].0)
        .execute(&ctx.db)
        .await
        .unwrap();
}

/// 공용 레퍼런스 카탈로그는 로그인한 아무 계정이 아니라 관리자만 고친다.
#[tokio::test]
async fn reference_catalog_writes_need_admin() {
    let Some(ctx) = setup().await else {
        eprintln!("TEST_DATABASE_URL 이 없어 건너뜁니다");
        return;
    };
    let missing = format!("/api/references/{}", uuid::Uuid::new_v4());
    let body = serde_json::json!({"name": "x", "description": "y"});
    let app = app_with_admins(&ctx, AuthMode::Strict, vec![ctx.users[0].0.clone()]);
    // 일반 계정(B): 만들기·고치기·지우기 모두 403 — 임베딩(유료) 호출 전에 막는다.
    for (method, path) in [
        ("POST", "/api/references"),
        ("PUT", missing.as_str()),
        ("DELETE", missing.as_str()),
    ] {
        let (st, _, _) = call(
            &app,
            method,
            path,
            Opts {
                token: tok(&ctx, 1),
                body: Some(body.clone()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{method} {path}");
    }
    // 관리자(A): 권한은 통과하고 없는 항목이라 404.
    let (st, _, _) = call(
        &app,
        "DELETE",
        &missing,
        Opts {
            token: tok(&ctx, 0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // 읽기는 공개.
    let (st, _, _) = call(&app, "GET", "/api/references", Opts::default()).await;
    assert_eq!(st, StatusCode::OK);
    // 개발 모드의 로컬 요청(default)은 관리자로 본다(기존 로컬 작업 흐름).
    let dev = app_with_admins(&ctx, AuthMode::DevDefaultLoopback, Vec::new());
    let (st, _, _) = call(
        &dev,
        "DELETE",
        &missing,
        Opts {
            peer: Some("127.0.0.1:1"),
            headers: vec![("host", "localhost:3003")],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = call(
        &dev,
        "DELETE",
        &missing,
        Opts {
            token: tok(&ctx, 1),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "개발 모드여도 다른 계정은 관리자가 아니다"
    );
}
