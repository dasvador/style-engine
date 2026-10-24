//! `/static`(생성 코디 사진) 보호. 사진은 사용자별 데이터라 공개 경로로 내보내지 않는다.
//!
//! 개발 모드(`WARDROBE_AUTH_MODE=dev-default-loopback`)에서 이 컴퓨터가 직접 보낸 요청만 그대로
//! 통과시킨다 — 로컬 화면이 `<img src="/static/images/...">` 로 쓰기 때문이다. 그 밖에는 404 다.
//! 토큰 사용자는 소유를 확인하는 `GET /api/images/{file}` 로 받는다.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::auth::{AuthMode, is_local_request};
use crate::AppState;

pub async fn local_dev_only(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    if state.auth_mode == AuthMode::DevDefaultLoopback && is_local_request(&parts) {
        return next.run(Request::from_parts(parts, body)).await;
    }
    StatusCode::NOT_FOUND.into_response()
}
