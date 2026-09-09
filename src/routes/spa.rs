//! React 빌드 결과(SPA)를 서빙한다.
//!
//! 운영에서는 Node 서버를 따로 두지 않고 Axum 하나가 정적 파일과 `/api` 를 같은 origin 에서
//! 제공한다. 그래서 프런트는 항상 상대 경로로 API 를 호출할 수 있고, CORS 도 필요 없다.
//!
//! 클라이언트 라우팅 때문에 `/wardrobe/<id>` 같은 경로로 직접 들어오거나 새로고침하면
//! 서버에 그 경로의 파일이 없다. 이때 `index.html` 을 **200 으로** 돌려주어 React Router 가
//! 경로를 해석하게 한다. `ServeDir::not_found_service` 는 404 상태를 그대로 유지하므로
//! (본문은 index.html 이어도 상태는 404) 여기서는 fallback 핸들러를 직접 쓴다.
//!
//! 정적 파일 위치는 `FRONTEND_DIST` 로 바꿀 수 있다. 기본값은 저장소 배치와 Docker 이미지
//! 배치가 같도록 `frontend/dist` 로 둔다.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use tower_http::services::ServeDir;

/// 정적 파일 루트. 환경변수로 재정의할 수 있다.
pub fn dist_dir() -> PathBuf {
    std::env::var("FRONTEND_DIST")
        .unwrap_or_else(|_| "frontend/dist".to_string())
        .into()
}

/// `dist` 를 서빙하고, 없는 경로는 `index.html`(200)로 넘긴다.
///
/// `/api` 는 이 라우터보다 먼저 매칭되므로 여기로 내려오지 않는다 — 즉 API 의 404 가
/// index.html 로 바뀌는 일은 없다.
pub fn router() -> Router {
    let dist = dist_dir();
    let index_path = dist.join("index.html");

    let Ok(index_html) = std::fs::read_to_string(&index_path) else {
        // 프런트를 빌드하지 않고 서버만 띄운 경우(예: API 개발).
        // 조용히 404 를 주는 대신 무엇을 해야 하는지 알려준다.
        tracing::warn!(
            path = %index_path.display(),
            "프런트엔드 빌드 결과가 없습니다. `cd frontend && npm run build` 를 실행하거나 \
             FRONTEND_DIST 를 설정하세요. API 는 정상 동작합니다."
        );
        return Router::new().fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                "프런트엔드가 빌드되지 않았습니다. `cd frontend && npm run build` 후 다시 시도하세요.",
            )
        });
    };

    // index.html 은 매 요청 디스크를 읽을 만큼 크지 않으므로 기동 시 한 번 읽어 둔다.
    let index_html = Arc::new(index_html);

    let serve_dir = ServeDir::new(&dist).fallback(axum::routing::any(move || {
        let html = Arc::clone(&index_html);
        async move { spa_index(&html) }
    }));

    Router::new().fallback_service(serve_dir)
}

/// SPA 진입점 응답. 상태는 200 이어야 하고, 라우팅이 클라이언트에 있으므로 캐시하지 않는다.
fn spa_index(html: &str) -> Response {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-cache")],
        Html(html.to_owned()),
    )
        .into_response()
}
