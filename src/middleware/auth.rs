use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
};

use crate::AppState;

/// 인증된 유저 ID. API 토큰으로 추출.
/// 토큰이 없으면 "default" 유저로 폴백 (하위호환).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        // Authorization: Bearer <token> 헤더에서 토큰 추출
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        match token {
            Some(token) => {
                // DB에서 토큰으로 유저 찾기
                let row =
                    sqlx::query_scalar::<_, String>("SELECT id FROM app_user WHERE api_token = ?")
                        .bind(token)
                        .fetch_optional(&app_state.db)
                        .await
                        .unwrap_or(None);

                match row {
                    Some(user_id) => Ok(AuthUser { user_id }),
                    None => Ok(AuthUser {
                        user_id: "default".to_string(),
                    }), // 잘못된 토큰 → default
                }
            }
            None => {
                // 토큰 없음 → default 유저 (하위호환)
                Ok(AuthUser {
                    user_id: "default".to_string(),
                })
            }
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
