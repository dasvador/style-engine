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

use std::path::{Path, PathBuf};

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

    if !index_path.is_file() {
        // 프런트를 빌드하지 않고 서버만 띄운 경우(예: API 개발).
        // 조용히 404 를 주는 대신 무엇을 해야 하는지 알려준다. 라우터는 그대로 세운다 —
        // 서버를 띄운 뒤에 빌드해도 재시작 없이 잡히도록.
        tracing::warn!(
            path = %index_path.display(),
            "프런트엔드 빌드 결과가 없습니다. `cd frontend && npm run build` 를 실행하거나 \
             FRONTEND_DIST 를 설정하세요. API 는 정상 동작합니다."
        );
    }

    let serve_dir = ServeDir::new(&dist).fallback(axum::routing::any(move || {
        let path = index_path.clone();
        async move { spa_index(&path).await }
    }));

    Router::new().fallback_service(serve_dir)
}

/// SPA 진입점 응답. 상태는 200 이어야 하고, 라우팅이 클라이언트에 있으므로 캐시하지 않는다.
///
/// 기동 시 한 번 읽어 두지 않고 요청마다 읽는다. Vite 는 asset 파일 이름에 해시를 붙이므로
/// 다시 빌드하면 index.html 이 가리키는 파일 이름이 바뀐다. 메모리에 들고 있으면 빌드 후
/// 새로고침해도 이미 사라진 옛 asset 을 계속 요청해 흰 화면이 되고, 서버를 재시작해야만
/// 풀린다. 1KB 남짓한 파일을 SPA 진입 요청마다 한 번 읽는 비용이 그보다 싸다
/// (해시가 붙은 asset 자체는 `ServeDir` 이 직접 서빙하므로 여기를 거치지 않는다).
async fn spa_index(index_path: &Path) -> Response {
    match tokio::fs::read_to_string(index_path).await {
        Ok(html) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-cache")],
            Html(html),
        )
            .into_response(),
        Err(err) => {
            tracing::warn!(
                path = %index_path.display(),
                error = %err,
                "index.html 을 읽을 수 없습니다."
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::CACHE_CONTROL, "no-store")],
                "프런트엔드가 빌드되지 않았습니다. `cd frontend && npm run build` 후 다시 시도하세요.",
            )
                .into_response()
        }
    }
}
