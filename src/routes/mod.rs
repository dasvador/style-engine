pub mod chat;
pub mod clothes;
pub mod feedback;
pub mod health;
pub mod outfit;
pub mod recommendation;
pub mod reference;
pub mod region;
pub mod spa;
pub mod style_mood;
pub mod user;
pub mod weather;

use axum::Router;

use crate::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .nest("/health", health::router())
        .nest("/clothes", clothes::router())
        .nest("/region", region::router())
        .nest("/weather", weather::router())
        .nest("/recommendation", recommendation::router())
        .nest("/references", reference::router())
        .nest("/outfit", outfit::router())
        .nest("/chat", chat::router())
        .nest("/feedback", feedback::router())
        .nest("/user", user::router())
        .nest("/style-moods", style_mood::router())
        // 정의되지 않은 /api 경로는 여기서 끝난다. 이 fallback 이 없으면 바깥 라우터의
        // SPA fallback 으로 내려가 index.html 이 200 으로 나가고, 클라이언트는 오타 난
        // 엔드포인트를 성공으로 오해한다.
        .fallback(api_not_found)
}

async fn api_not_found(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "error": format!("알 수 없는 API 경로입니다: {}", uri.path()),
        })),
    )
}

/// React 빌드 결과(SPA) 라우터. 상태를 쓰지 않는다.
pub fn spa_router() -> Router<AppState> {
    spa::router().with_state(())
}
