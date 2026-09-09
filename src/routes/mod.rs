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

/// 요청에 담겨 온 장르 문자열을 표준 식별자로 바꾼다.
///
/// 예전 클라이언트가 `quiet_luxury` 같은 옛 이름을 보낼 수 있으므로 별칭을 받아준다.
/// 다만 모르는 값은 400 으로 돌려준다 — 조용히 `None` 으로 떨어뜨리면 필터가 걸리지
/// 않은 채 옷장 전체가 후보로 들어가고, 사용자는 자기가 고른 장르가 무시된 줄 모른다.
pub fn parse_genre(
    raw: Option<&str>,
) -> Result<Option<crate::models::style_vocab::StyleGenre>, crate::errors::AppError> {
    use crate::models::style_vocab::StyleGenre;

    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => StyleGenre::from_alias(s).map(Some).ok_or_else(|| {
            crate::errors::AppError::BadRequest(format!(
                "알 수 없는 스타일 장르입니다: '{s}'. 허용: {}",
                StyleGenre::ALL
                    .iter()
                    .map(|g| g.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }),
    }
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
