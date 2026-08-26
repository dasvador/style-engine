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

    // Seed reference data if empty, then load cache
    embedding_service
        .seed_if_empty(&pool)
        .await
        .expect("Failed to seed reference data");
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
    };

    // Build router
    let app = Router::new()
        .merge(routes::home_router())
        .nest("/api", routes::api_router())
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3003));
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
