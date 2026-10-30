//! MCP 서버(같은 컴퓨터)가 연결된 계정 대신 추천을 부를 때 쓰는 위임 헤더.
//!
//! ChatGPT 계정 연결(OAuth)은 `mcp/` 가 맡고, 그 토큰은 이 서버의 `api_token` 과 별개다. 기존 `default`
//! 계정은 토큰이 무효화돼 있어 `api_token` 을 대신 실어 보낼 수도 없다. 그래서 MCP 서버가 OAuth 로 확인한
//! 주체를 공유 키(`WARDROBE_DELEGATION_KEY`)로 서명해 보내고, 여기서 검증한다.
//!
//! 받아들이는 조건(모두 만족해야 한다):
//!   - 서버에 키가 설정돼 있다(없으면 위임은 꺼져 있다).
//!   - 요청이 이 컴퓨터에서 직접 왔다(`auth::is_local_request` — 루프백 연결·로컬 Host·전달 헤더 없음).
//!   - `Authorization` 헤더가 같이 오지 않았다(둘 중 무엇을 믿을지 고르지 않는다).
//!   - 허용된 경로 하나(`POST /api/recommendation/multi`)다. 서명에도 메서드·경로가 들어간다.
//!   - 만료 전이고 만료가 지금부터 [`MAX_TTL_SECS`] 안이다.
//!   - HMAC-SHA256 서명이 맞다(상수 시간 비교).
//!   - 서명된 사용자가 `app_user` 에 있다(여기서는 형식만 보고, 존재 확인은 `auth` 가 한다).
//!
//! 헤더 값: `v1.<base64url(user_id)>.<만료 unix 초>.<hex(서명)>`
//! 서명 대상: `wardrobe-delegation/v1\n<METHOD>\n<경로>\n<user_id>\n<만료>`

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const HEADER: &str = "x-wardrobe-delegation";
/// 서명의 최대 수명. MCP 는 요청 직전에 서명한다.
pub const MAX_TTL_SECS: i64 = 120;
/// 위임으로 부를 수 있는 (메서드, 경로).
pub const ALLOWED: &[(&str, &str)] = &[("POST", "/api/recommendation/multi")];
/// 키 최소 길이(바이트).
pub const MIN_KEY_LEN: usize = 32;

/// 환경 변수 `WARDROBE_DELEGATION_KEY`(hex). 없거나 짧거나 hex 가 아니면 위임을 끈다.
pub fn key_from_env() -> Option<std::sync::Arc<[u8]>> {
    let raw = std::env::var("WARDROBE_DELEGATION_KEY").ok()?;
    match hex::decode(raw.trim()) {
        Ok(k) if k.len() >= MIN_KEY_LEN => Some(k.into()),
        _ => {
            tracing::warn!("WARDROBE_DELEGATION_KEY 가 32바이트 이상 hex 가 아니라 위임을 끈다");
            None
        }
    }
}

fn message(method: &str, path: &str, user_id: &str, exp: i64) -> String {
    format!("wardrobe-delegation/v1\n{method}\n{path}\n{user_id}\n{exp}")
}

/// 서명(테스트용). MCP 쪽 `mcp/src/recommend.js` 와 같은 형식이다.
#[cfg(test)]
pub fn sign(key: &[u8], method: &str, path: &str, user_id: &str, exp: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC 은 어떤 길이의 키도 받는다");
    mac.update(message(method, path, user_id, exp).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    let uid = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(user_id);
    format!("v1.{uid}.{exp}.{sig}")
}

/// 위임 헤더를 검증해 사용자 id 를 돌려준다. 실패 이유는 로그·`WWW-Authenticate` 용 짧은 ASCII.
pub fn verify(
    key: &[u8],
    header: &str,
    method: &str,
    path: &str,
    now: i64,
) -> Result<String, &'static str> {
    if !ALLOWED.iter().any(|(m, p)| *m == method && *p == path) {
        return Err("delegation not allowed for this route");
    }
    let mut parts = header.trim().split('.');
    let (Some("v1"), Some(uid), Some(exp), Some(sig), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return Err("malformed delegation");
    };
    let user_id = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(uid)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|u| !u.is_empty() && u.len() <= 50)
        .ok_or("malformed delegation")?;
    let exp: i64 = exp.parse().map_err(|_| "malformed delegation")?;
    if exp < now || exp > now + MAX_TTL_SECS {
        return Err("delegation expired");
    }
    let sig = hex::decode(sig).map_err(|_| "malformed delegation")?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| "bad key")?;
    mac.update(message(method, path, &user_id, exp).as_bytes());
    mac.verify_slice(&sig)
        .map_err(|_| "bad delegation signature")?;
    Ok(user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
    const P: &str = "/api/recommendation/multi";

    #[test]
    fn accepts_only_fresh_signed_header_for_allowed_route() {
        let now = 1_000_000;
        let h = sign(KEY, "POST", P, "user-a", now + 60);
        assert_eq!(verify(KEY, &h, "POST", P, now).as_deref(), Ok("user-a"));

        // 다른 키·경로·메서드·만료
        assert!(verify(b"another-key-another-key-another-k", &h, "POST", P, now).is_err());
        assert!(verify(KEY, &h, "POST", "/api/clothes", now).is_err());
        assert!(verify(KEY, &h, "GET", P, now).is_err());
        assert!(verify(KEY, &h, "POST", P, now + 61).is_err());
        // 허용 경로가 아니면 그 경로로 서명했어도 거절
        let other = sign(KEY, "GET", "/api/clothes", "user-a", now + 60);
        assert!(verify(KEY, &other, "GET", "/api/clothes", now).is_err());
        // 수명이 너무 긴 서명
        let long = sign(KEY, "POST", P, "user-a", now + MAX_TTL_SECS + 1);
        assert!(verify(KEY, &long, "POST", P, now).is_err());
    }

    /// MCP(Node) `signDelegation` 이 만든 값과 같다 — 두 구현이 같은 형식을 쓴다.
    #[test]
    fn matches_mcp_signature_vector() {
        const NODE: &str = "v1.ZGVmYXVsdA.1800000000.71ecebacc13664bf399a72cb679807db0bd752240b75973cb57d5e37d30b2122";
        assert_eq!(sign(KEY, "POST", P, "default", 1_800_000_000), NODE);
        assert_eq!(
            verify(KEY, NODE, "POST", P, 1_800_000_000 - 30).as_deref(),
            Ok("default")
        );
    }

    #[test]
    fn user_id_cannot_be_swapped() {
        let now = 1_000_000;
        let h = sign(KEY, "POST", P, "user-a", now + 60);
        let parts: Vec<&str> = h.split('.').collect();
        let b = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("user-b");
        let swapped = format!("v1.{b}.{}.{}", parts[2], parts[3]);
        assert!(verify(KEY, &swapped, "POST", P, now).is_err());
        for bad in [
            "",
            "v1",
            "v2.a.1.00",
            "v1.%%.1.00",
            "v1.dXNlcg.x.00",
            "v1.dXNlcg.1.zz.extra",
        ] {
            assert!(verify(KEY, bad, "POST", P, now).is_err(), "{bad}");
        }
    }
}
