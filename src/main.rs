use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use sqlx::mysql::MySqlPoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod db;
mod errors;
mod middleware;
mod models;
mod routes;
mod services;

use services::embedding::EmbeddingService;
use services::llm::LlmClient;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::MySqlPool,
    pub http_client: reqwest::Client,
    /// 모든 모델 호출의 단일 진입점. task → provider/model 라우팅, 재시도, 계측을 담당한다.
    pub llm: Arc<LlmClient>,
    pub kma_api_key: String,
    pub embedding: Arc<EmbeddingService>,
    /// 토큰 없는 요청을 어떻게 볼지(`middleware::auth`). 기본은 거절.
    pub auth_mode: middleware::auth::AuthMode,
    /// 공용 카탈로그 관리자(`WARDROBE_ADMIN_USERS`).
    pub admin_users: Arc<Vec<String>>,
    /// MCP 서버 위임 헤더 키(`WARDROBE_DELEGATION_KEY`). 없으면 위임을 받지 않는다.
    pub delegation_key: Option<Arc<[u8]>>,
}

#[tokio::main]
async fn main() {
    // Load .env
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "style_engine=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Database connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let kma_api_key = std::env::var("KMA_API_KEY").unwrap_or_default();
    let http_client = reqwest::Client::new();

    // LLM provider 레이어. task별 provider/model 라우팅을 환경변수에서 읽는다.
    let llm = Arc::new(LlmClient::from_env(http_client.clone()));

    // Initialize embedding service (API 기반 — 로컬 RAM 사용 없음)
    let embedding_service = Arc::new(
        EmbeddingService::new(llm.clone()).expect("Failed to initialize embedding service"),
    );

    // 레퍼런스: 카탈로그 업서트 → 캐시 적재.
    //
    // 순서가 중요하다. `sync_catalog` 는 설명이 바뀐 행의 `embedding` 을 NULL 로
    // 되돌리고, 뒤이어 도는 `load_cache` 가 NULL 인 것만 임베딩해 저장한다. 순서를
    // 뒤집으면 바뀐 설명이 옛 벡터로 검색된다.
    //
    // 예전에는 여기서 `seed_if_empty` 가 먼저 돌았다. 그 함수는 표가 비어 있을 때만
    // 전체를 넣고 한 행이라도 있으면 건너뛰어서, 레퍼런스를 추가해도 기존 DB 에는
    // 반영되지 않았다. 카탈로그 업서트가 그 역할까지 대신한다 — 빈 표든 아니든 같다.
    services::reference_catalog::sync_catalog(&pool)
        .await
        .expect("레퍼런스 카탈로그 반영 실패");
    embedding_service
        .load_cache(&pool)
        .await
        .expect("Failed to load reference cache");

    let state = AppState {
        db: pool,
        http_client,
        llm,
        kma_api_key,
        embedding: embedding_service,
        auth_mode: middleware::auth::AuthMode::from_env(),
        admin_users: middleware::auth::admin_users_from_env(),
        delegation_key: middleware::delegation::key_from_env(),
    };
    if state.delegation_key.is_some() {
        tracing::info!("MCP 위임: 켜짐(로컬 직접 요청·추천 경로만)");
    }
    match state.auth_mode {
        middleware::auth::AuthMode::Strict => {
            tracing::info!(
                "인증: 토큰 필요(기본). 로컬 화면 개발은 WARDROBE_AUTH_MODE=dev-default-loopback"
            )
        }
        middleware::auth::AuthMode::DevDefaultLoopback => tracing::warn!(
            "인증: 개발 모드 — 이 컴퓨터에서 직접 온 토큰 없는 요청은 default 사용자. 터널로 이 포트를 노출하지 마세요."
        ),
    }

    // Build router
    // 라우팅 순서가 중요하다. /api 와 /static 이 먼저 매칭되고, 나머지 모든 경로는
    // SPA 라우터의 fallback 으로 내려가 index.html 을 받는다.
    let app = Router::new()
        .nest("/api", routes::api_router())
        // 생성 코디 사진(static/images)은 사용자별 데이터라 공개 경로로 내보내지 않는다. 개발 모드의
        // 로컬 요청만 그대로 읽고, 토큰 사용자는 소유 확인을 거치는 /api/images/{file} 로 받는다.
        .nest_service(
            "/static",
            axum::Router::new()
                .fallback_service(tower_http::services::ServeDir::new("static"))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    middleware::static_guard::local_dev_only,
                ))
                .with_state(()),
        )
        .merge(routes::spa_router())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    // 기본은 예전처럼 모든 인터페이스. `WARDROBE_BIND_HOST=127.0.0.1` 이면 이 컴퓨터에서만 받는다.
    let host: std::net::IpAddr = std::env::var("WARDROBE_BIND_HOST")
        .ok()
        .and_then(|h| h.trim().parse().ok())
        .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    let addr = SocketAddr::new(host, 3003);
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    // 연결 상대 주소를 요청에 붙인다 — 개발 모드의 로컬 판정(`is_local_request`)에 쓴다.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
