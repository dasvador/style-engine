//! 요청한 사용자(`AuthUser`). 개인 데이터(옷장·비교·피드백·룩북·채팅 기록)는 모두 이 값에 묶인다.
//!
//! 기본은 닫혀 있다(fail closed):
//!   - `Authorization: Bearer <api_token>` 이 맞으면 그 사용자.
//!   - 헤더가 있는데 형식이 틀리거나 토큰이 없는 값이면 401. DB 를 못 읽으면 503 — 어느 경우도
//!     `default` 로 바꾸지 않는다.
//!   - 헤더가 없으면 401.
//!
//! 예외는 하나뿐이다. `WARDROBE_AUTH_MODE=dev-default-loopback` 을 명시했고, 요청이 이 컴퓨터에서
//! 직접 온 것일 때만(`is_local_request`) 토큰 없이 `default` 사용자로 본다. 로컬 화면(토큰을 보내지
//! 않는다) 개발용이다. 터널·프록시를 거친 요청이나 다른 웹사이트가 브라우저로 보낸 요청은 여기
//! 해당하지 않는다.
//!
//! 따로 MCP 서버의 위임 헤더(`middleware::delegation`)가 있다. 서명이 맞고 로컬 직접 요청이며 허용
//! 경로(추천)일 때만 그 사용자다. 틀리면 401 — 개발 모드여도 `default` 로 넘어가지 않는다.

use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};

use crate::AppState;

/// 토큰 없이 쓰는 로컬 개발 사용자. 기존 옷장·기록이 이 id 로 남아 있다.
pub const DEFAULT_USER_ID: &str = "default";

/// 인증 방식. 환경 변수 `WARDROBE_AUTH_MODE` 로 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// 토큰이 있어야 한다(기본값).
    Strict,
    /// 로컬 개발: 이 컴퓨터에서 직접 온 토큰 없는 요청만 `default`.
    DevDefaultLoopback,
}

impl AuthMode {
    pub fn from_env() -> Self {
        match std::env::var("WARDROBE_AUTH_MODE").as_deref() {
            Ok("dev-default-loopback") => AuthMode::DevDefaultLoopback,
            _ => AuthMode::Strict,
        }
    }
}

/// 공용 데이터(레퍼런스 카탈로그) 관리자 목록. 환경 변수 `WARDROBE_ADMIN_USERS`(쉼표 구분 사용자 id).
pub fn admin_users_from_env() -> std::sync::Arc<Vec<String>> {
    std::sync::Arc::new(
        std::env::var("WARDROBE_ADMIN_USERS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// 공용 카탈로그를 고칠 수 있는가. 로그인한 아무 계정(새로 가입한 계정 포함)이 아니라:
///   - `WARDROBE_ADMIN_USERS` 에 적힌 계정, 또는
///   - 개발 모드에서 이 컴퓨터가 직접 보낸 요청(= `default`). `default` 의 토큰은 무효화돼 있어
///     원격에서 이 신원으로 들어올 수 없다.
pub fn require_admin(state: &AppState, auth: &AuthUser) -> Result<(), crate::errors::AppError> {
    let listed = state.admin_users.iter().any(|u| u == &auth.user_id);
    let local_dev =
        state.auth_mode == AuthMode::DevDefaultLoopback && auth.user_id == DEFAULT_USER_ID;
    if listed || local_dev {
        Ok(())
    } else {
        Err(crate::errors::AppError::Forbidden(
            "공용 카탈로그는 관리자만 고칠 수 있어요.".to_string(),
        ))
    }
}

/// 인증된 사용자 ID.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

/// 토큰 최대 길이. `app_user.api_token` 이 VARCHAR(64) 다.
const MAX_TOKEN_LEN: usize = 128;

fn unauthorized(description: &str) -> Response {
    let mut res = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "로그인이 필요합니다." })),
    )
        .into_response();
    let challenge = format!("Bearer error=\"invalid_token\", error_description=\"{description}\"");
    if let Ok(v) = HeaderValue::from_str(&challenge) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

/// 요청이 이 컴퓨터에서 직접 왔는가. 다음을 모두 만족해야 한다:
///   - 연결 상대가 루프백 주소(서버를 `connect_info` 와 함께 띄웠을 때만 알 수 있다)
///   - Host 가 localhost·127.0.0.1·[::1] (터널은 공개 호스트 이름을 넘긴다)
///   - 프록시·터널이 붙이는 전달 헤더가 없음
///   - Origin 이 없거나 로컬 주소 (다른 웹사이트가 사용자의 브라우저로 보낸 요청 차단)
pub fn is_local_request(parts: &Parts) -> bool {
    let peer_is_loopback = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback());
    peer_is_loopback && is_local_headers(&parts.headers)
}

fn is_local_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [::1]:3003
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map_or(host, |(h, _)| h)
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

fn is_local_headers(headers: &HeaderMap) -> bool {
    const FORWARDING: [&str; 6] = [
        "forwarded",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-real-ip",
        "cf-connecting-ip",
        "true-client-ip",
    ];
    if FORWARDING.iter().any(|h| headers.contains_key(*h)) {
        return false;
    }
    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(is_local_host);
    let origin_ok = match headers.get(header::ORIGIN) {
        None => true,
        Some(v) => v
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(is_local_host),
    };
    host_ok && origin_ok
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        if let Some(raw) = parts.headers.get(super::delegation::HEADER) {
            return delegated_user(parts, &app_state, raw).await;
        }

        let Some(raw) = parts.headers.get(header::AUTHORIZATION) else {
            if app_state.auth_mode == AuthMode::DevDefaultLoopback && is_local_request(parts) {
                return Ok(AuthUser {
                    user_id: DEFAULT_USER_ID.to_string(),
                });
            }
            return Err(unauthorized("missing token"));
        };

        let token = raw
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN_LEN && !t.starts_with("revoked-"));
        let Some(token) = token else {
            return Err(unauthorized("malformed token"));
        };

        match sqlx::query_scalar::<_, String>("SELECT id FROM app_user WHERE api_token = ?")
            .bind(token)
            .fetch_optional(&app_state.db)
            .await
        {
            Ok(Some(user_id)) => Ok(AuthUser { user_id }),
            Ok(None) => Err(unauthorized("unknown token")),
            Err(e) => {
                tracing::error!("인증 조회 실패: {e}");
                Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": "잠시 후 다시 시도해 주세요." })),
                )
                    .into_response())
            }
        }
    }
}

/// 위임 헤더 처리. 어떤 실패도 `default` 로 바꾸지 않고 401(DB 오류면 503)이다.
// 오류 형식은 추출기의 Rejection(`Response`)과 같게 둔다.
#[allow(clippy::result_large_err)]
async fn delegated_user(
    parts: &Parts,
    app_state: &AppState,
    raw: &HeaderValue,
) -> Result<AuthUser, Response> {
    let Some(key) = app_state.delegation_key.as_deref() else {
        return Err(unauthorized("delegation disabled"));
    };
    if parts.headers.contains_key(header::AUTHORIZATION) {
        return Err(unauthorized("ambiguous credentials"));
    }
    if !is_local_request(parts) {
        return Err(unauthorized("delegation only from this machine"));
    }
    // nest 안에서는 parts.uri 가 접두어를 뗀 경로라 원래 경로를 쓴다.
    let path = parts
        .extensions
        .get::<axum::extract::OriginalUri>()
        .map_or_else(|| parts.uri.path().to_string(), |u| u.0.path().to_string());
    let now = chrono::Utc::now().timestamp();
    let user_id = match raw
        .to_str()
        .map_err(|_| "malformed delegation")
        .and_then(|h| super::delegation::verify(key, h, parts.method.as_str(), &path, now))
    {
        Ok(u) => u,
        Err(reason) => {
            tracing::warn!(reason, "위임 헤더 거절");
            return Err(unauthorized(reason));
        }
    };
    match sqlx::query_scalar::<_, String>("SELECT id FROM app_user WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&app_state.db)
        .await
    {
        Ok(Some(user_id)) => Ok(AuthUser { user_id }),
        Ok(None) => Err(unauthorized("unknown user")),
        Err(e) => {
            tracing::error!("인증 조회 실패: {e}");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "잠시 후 다시 시도해 주세요." })),
            )
                .into_response())
        }
    }
}

/// AppState에서 FromRef 구현
trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

impl FromRef<AppState> for AppState {
    fn from_ref(input: &AppState) -> Self {
        input.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn local_headers_only_for_direct_local_requests() {
        assert!(is_local_headers(&headers(&[("host", "localhost:3003")])));
        assert!(is_local_headers(&headers(&[("host", "127.0.0.1:3003")])));
        assert!(is_local_headers(&headers(&[("host", "[::1]:3003")])));
        assert!(is_local_headers(&headers(&[
            ("host", "localhost:3003"),
            ("origin", "http://localhost:5173")
        ])));
        // 터널: 공개 호스트 이름
        assert!(!is_local_headers(&headers(&[(
            "host",
            "abc.trycloudflare.com"
        )])));
        // 프록시 전달 헤더
        assert!(!is_local_headers(&headers(&[
            ("host", "localhost:3003"),
            ("x-forwarded-for", "203.0.113.5")
        ])));
        assert!(!is_local_headers(&headers(&[
            ("host", "localhost:3003"),
            ("cf-connecting-ip", "203.0.113.5")
        ])));
        // 다른 웹사이트가 사용자 브라우저로 보낸 요청
        assert!(!is_local_headers(&headers(&[
            ("host", "localhost:3003"),
            ("origin", "https://evil.example")
        ])));
        assert!(!is_local_headers(&headers(&[
            ("host", "localhost:3003"),
            ("origin", "http://localhost.evil.example")
        ])));
        // Host 없음·속임수
        assert!(!is_local_headers(&headers(&[])));
        assert!(!is_local_headers(&headers(&[(
            "host",
            "localhost.evil.example"
        )])));
    }
}
