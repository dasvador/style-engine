//! 레퍼런스 지식 베이스 카탈로그.
//!
//! 레퍼런스 내용을 Rust 소스가 아니라 `data/clothing_references.toml` 에 두고
//! `include_str!` 로 박는다. 이유는 두 가지다.
//!
//! - **검토 가능해야 한다.** 설명은 긴 한국어 산문이고, 틀린 문장이 있어도 코드는
//!   오류를 내지 않는다 — 그대로 Pass 2 프롬프트에 들어가 Vision 의 판단 근거가 되고
//!   결과는 "조금 더 틀린 분석" 으로만 나타난다. 사람이 읽고 고칠 수 있는 형태여야
//!   한다.
//! - **컴파일 시점에 들어 있어야 한다.** 런타임에 파일을 찾지 않으므로 배포에서
//!   파일이 빠져 레퍼런스가 조용히 사라지는 일이 없다.
//!
//! 적용은 **이름을 키로 한 업서트**다. `seed_if_empty` 와 달리 표가 비어 있지 않아도
//! 돈다 — 기존 방식은 `count > 0` 이면 전체를 건너뛰어서, 이미 13행이 있는 DB 에는
//! 레퍼런스를 추가해도 영원히 반영되지 않았다.

use serde::Deserialize;
use sqlx::MySqlPool;

use crate::db::reference_repo::{self, NewReference};
use crate::models::style_vocab::StyleGenre;

/// 카탈로그 원문. 빌드에 포함된다.
const CATALOG_TOML: &str = include_str!("../../data/clothing_references.toml");

#[derive(Debug, Deserialize)]
struct Catalog {
    #[serde(default)]
    references: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    name: String,
    category: String,
    #[serde(default)]
    subcategory: Option<String>,
    #[serde(default)]
    era: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    /// `approved` | `draft`. 파일에 적는다 — 기본값에 기대지 않는다.
    review_status: String,
    #[serde(default)]
    source_note: Option<String>,
    /// 검색용 축약 표현. 관찰 가능한 형태·소재·디테일만 담는다.
    ///
    /// 없으면 `description` 전체를 임베딩한다. 기존 시드 13개가 그렇다 — 그 13개에
    /// 쓴 검색 문장은 검수 전이라 적용하지 않았다(카탈로그의 해당 절 주석 참고).
    #[serde(default)]
    embedding_text: Option<String>,
    /// Pass 2 가 읽는 설명.
    description: String,
}

/// 카탈로그를 파싱하고 값을 검증한다.
///
/// 어휘 밖의 값을 통과시키지 않는 이유: 틀린 카테고리는 선필터에서 어느 질의와도
/// 일치하지 않아 그 레퍼런스가 조용히 죽는다. 틀린 장르는 커버리지 통계를 틀리게
/// 만든다. 둘 다 오류로 보이지 않으므로 여기서 막는다.
fn parse_catalog(raw: &str) -> anyhow::Result<Vec<CatalogEntry>> {
    const KNOWN_CATEGORIES: [&str; 8] = [
        "상의",
        "하의",
        "아우터",
        "신발",
        "가방",
        "액세서리",
        "모자",
        "벨트",
    ];

    let catalog: Catalog = toml::from_str(raw)?;
    let mut seen: Vec<&str> = Vec::with_capacity(catalog.references.len());

    for e in &catalog.references {
        if !KNOWN_CATEGORIES.contains(&e.category.as_str()) {
            anyhow::bail!("'{}': 알 수 없는 카테고리 '{}'", e.name, e.category);
        }
        if !matches!(e.review_status.as_str(), "approved" | "draft") {
            anyhow::bail!(
                "'{}': review_status 는 approved 또는 draft 여야 한다 (받은 값 '{}')",
                e.name,
                e.review_status
            );
        }
        for g in &e.genres {
            if g.parse::<StyleGenre>().is_err() {
                anyhow::bail!("'{}': 알 수 없는 장르 '{}'", e.name, g);
            }
        }
        // 없는 것은 허용하지만 빈 것은 막는다. 빈 문자열을 임베딩하면 모든 질의에
        // 비슷한 유사도로 걸려서 검색 결과를 흐린다.
        if e.embedding_text
            .as_deref()
            .is_some_and(|t| t.trim().is_empty())
        {
            anyhow::bail!("'{}': embedding_text 가 비어 있다", e.name);
        }
        // 이름은 DB 의 유니크 키다. 파일 안에서 겹치면 뒤의 것이 앞의 것을 덮어쓰고,
        // 그 사실이 로그에도 남지 않는다.
        if seen.contains(&e.name.as_str()) {
            anyhow::bail!("'{}': 카탈로그 안에서 이름이 중복된다", e.name);
        }
        seen.push(&e.name);
    }

    Ok(catalog.references)
}

/// 카탈로그를 DB 에 반영한다. 여러 번 실행해도 결과가 같다.
///
/// 임베딩은 만들지 않는다 — 설명이 바뀐 행은 `upsert_reference` 가 `embedding` 을
/// NULL 로 되돌리고, 뒤이어 도는 `load_cache` 가 필요한 것만 생성해 저장한다.
/// 그래서 내용이 그대로인 레퍼런스에는 임베딩 API 호출이 한 건도 나가지 않는다.
pub async fn sync_catalog(pool: &MySqlPool) -> anyhow::Result<usize> {
    let entries = parse_catalog(CATALOG_TOML)?;
    if entries.is_empty() {
        return Ok(0);
    }

    for e in &entries {
        reference_repo::upsert_reference(
            pool,
            &NewReference {
                name: &e.name,
                category: Some(&e.category),
                subcategory: e.subcategory.as_deref(),
                era: e.era.as_deref(),
                style: e.style.as_deref(),
                description: e.description.trim(),
                embedding_text: e.embedding_text.as_deref().map(str::trim),
                review_status: &e.review_status,
                source_note: e.source_note.as_deref(),
                genres: &e.genres,
            },
        )
        .await?;
    }

    let drafts = entries
        .iter()
        .filter(|e| e.review_status == "draft")
        .count();
    tracing::info!(
        total = entries.len(),
        drafts,
        "레퍼런스 카탈로그 반영 완료 (draft 는 업로드 분석에서 검색되지 않는다)"
    );

    // 업서트는 지우지 않는다. 카탈로그에서 뺀 레퍼런스는 DB 에 그대로 남아 계속
    // 검색된다 — 지우는 것은 사람이 판단할 일이라 자동으로 하지 않고, 수가 어긋나는
    // 것만 알린다. 이게 없으면 카탈로그를 고쳐도 옛 레퍼런스가 조용히 살아 있다.
    let in_db = reference_repo::count_references(pool).await?;
    if in_db > entries.len() as i64 {
        tracing::warn!(
            in_db,
            in_catalog = entries.len(),
            "DB 에 카탈로그에 없는 레퍼런스가 있다. 의도한 것이 아니면 확인할 것"
        );
    }

    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 빌드에 들어간 카탈로그 자체가 규칙을 지키는지 본다. 실패하면 CI 가 잡는다.
    #[test]
    fn the_shipped_catalog_is_valid() {
        let entries = parse_catalog(CATALOG_TOML).expect("카탈로그 파싱 실패");
        assert!(!entries.is_empty(), "카탈로그가 비어 있다");
    }

    #[test]
    fn an_unknown_category_is_rejected() {
        let raw = r#"
[[references]]
name = "테스트"
category = "원피스"
review_status = "draft"
embedding_text = "검색 문장"
description = "설명"
"#;
        let err = parse_catalog(raw).unwrap_err().to_string();
        assert!(err.contains("알 수 없는 카테고리"), "{err}");
    }

    #[test]
    fn an_unknown_genre_is_rejected() {
        let raw = r#"
[[references]]
name = "테스트"
category = "상의"
genres = ["quiet_luxury"]
review_status = "draft"
embedding_text = "검색 문장"
description = "설명"
"#;
        let err = parse_catalog(raw).unwrap_err().to_string();
        assert!(err.contains("알 수 없는 장르"), "{err}");
    }

    #[test]
    fn a_duplicate_name_is_rejected() {
        let raw = r#"
[[references]]
name = "같은 이름"
category = "상의"
review_status = "draft"
embedding_text = "검색 문장"
description = "설명"

[[references]]
name = "같은 이름"
category = "하의"
review_status = "draft"
embedding_text = "검색 문장"
description = "설명"
"#;
        let err = parse_catalog(raw).unwrap_err().to_string();
        assert!(err.contains("이름이 중복"), "{err}");
    }

    /// 기존 시드는 검색 문장 없이 설명 전체를 임베딩한다.
    #[test]
    fn a_missing_search_text_is_allowed() {
        let raw = r#"
[[references]]
name = "테스트"
category = "상의"
review_status = "approved"
description = "설명"
"#;
        let entries = parse_catalog(raw).expect("검색 문장이 없어도 통과해야 한다");
        assert!(entries[0].embedding_text.is_none());
    }

    #[test]
    fn a_blank_search_text_is_rejected() {
        let raw = r#"
[[references]]
name = "테스트"
category = "상의"
review_status = "draft"
embedding_text = "   "
description = "설명"
"#;
        let err = parse_catalog(raw).unwrap_err().to_string();
        assert!(err.contains("embedding_text 가 비어"), "{err}");
    }

    #[test]
    fn an_unreviewed_status_is_rejected() {
        let raw = r#"
[[references]]
name = "테스트"
category = "상의"
review_status = "published"
embedding_text = "검색 문장"
description = "설명"
"#;
        let err = parse_catalog(raw).unwrap_err().to_string();
        assert!(err.contains("review_status"), "{err}");
    }
}
