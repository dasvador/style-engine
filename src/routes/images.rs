//! 생성 코디 사진을 소유자에게만 내보낸다.
//!
//!   GET /api/images/{file}   — `file` 은 `static/images/` 바로 아래의 `<uuid>.png` 이름
//!
//! 내 사진인지는 생성 사진 캐시의 소유자로만 가린다: `outfit_image.user_id = 나` 일 때만 준다.
//! 룩북(`lookbook_look.image_path`)은 근거로 쓰지 않는다 — 예전에는 룩북 저장·핀이 계정을 가리지 않아
//! 남의 사진 경로가 내 룩에 붙을 수 있었고, 그 경로가 곧 접근권이 되는 우회가 있었다.
//! 그 밖(남의 사진·없는 사진·이상한 이름)은 모두 404 다 — 있는지 없는지도 알려 주지 않는다.
//! 응답은 브라우저·프록시에 남지 않게 `private, no-store` 와 `Vary: Authorization` 을 붙인다(로그인 계정이
//! 바뀌어도 앞 계정의 사진이 캐시에서 나오지 않게).
//! 공개 경로 `/static` 은 개발 모드의 로컬 요청에만 열려 있다(`middleware::static_guard`).

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::AppState;
use crate::errors::AppError;
use crate::middleware::auth::AuthUser;

pub fn router() -> Router<AppState> {
    Router::new().route("/{file}", get(show))
}

/// 서버가 만든 이름(`<uuid v4>.png`)만 받는다. 경로 구분자·`..` 는 들어올 수 없다.
fn valid_name(file: &str) -> bool {
    file.strip_suffix(".png")
        .is_some_and(|stem| uuid::Uuid::parse_str(stem).is_ok() && stem.len() == 36)
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(file): Path<String>,
) -> Result<Response, AppError> {
    let not_found = || AppError::NotFound("사진을 찾을 수 없어요.".to_string());
    if !valid_name(&file) {
        return Err(not_found());
    }
    let image_path = format!("/static/images/{file}");
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM outfit_image WHERE image_path = ? AND user_id = ?)",
    )
    .bind(&image_path)
    .bind(&auth.user_id)
    .fetch_one(&state.db)
    .await?;
    if !owned {
        return Err(not_found());
    }

    let path = std::path::Path::new("static/images").join(&file);
    // 심볼릭 링크는 따라가지 않는다.
    match tokio::fs::symlink_metadata(&path).await {
        Ok(meta) if meta.is_file() => {}
        _ => return Err(not_found()),
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| not_found())?;
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err(not_found());
    }
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Body::from(bytes),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn only_server_made_names() {
        assert!(valid_name("0f1e2d3c-4b5a-4697-a8b9-cadbecfd0011.png"));
        for bad in [
            "../.env",
            "..%2f.env",
            "x.png",
            "0f1e2d3c-4b5a-4697-a8b9-cadbecfd0011.jpg",
            "0f1e2d3c4b5a4697a8b9cadbecfd0011.png",
            "user-0f1e2d3c-4b5a-4697-a8b9-cadbecfd0011.png",
            "0f1e2d3c-4b5a-4697-a8b9-cadbecfd0011.png/..",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
