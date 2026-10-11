use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// DB row for clothing_reference table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ClothingReference {
    pub id: String,
    pub name: String,
    /// `clothing.category` 와 같은 어휘(상의/하의/아우터/신발/가방/…).
    /// 검색 전 후보를 같은 종류로 좁히는 데 쓴다.
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    /// 검색용 축약 표현. `None` 이면 `description` 을 임베딩한다.
    ///
    /// 왜 나누는가: `description` 은 Pass 2 가 읽는 설명이라 브랜드·연혁이 섞여
    /// 있다. 검색은 관찰 가능한 형태·소재·디테일로 해야 하고, 그 둘을 한 벡터에
    /// 넣으면 "주요 브랜드: ..." 문장이 유사도를 흐린다.
    pub embedding_text: Option<String>,
    pub embedding: Option<serde_json::Value>,
    /// `approved` | `draft`. 업로드 분석 경로는 승인된 것만 검색한다.
    pub review_status: String,
    pub source_note: Option<String>,
    pub version: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl ClothingReference {
    /// 임베딩할 텍스트. `embedding_text` 가 있으면 그것, 없으면 설명 전체.
    pub fn text_for_embedding(&self) -> &str {
        self.embedding_text
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(&self.description)
    }
}

/// Request body for creating a reference
#[derive(Debug, Deserialize)]
pub struct CreateReferenceRequest {
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
}

/// Response DTO
#[derive(Debug, Serialize)]
pub struct ReferenceResponse {
    pub id: String,
    pub name: String,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// A matched reference with similarity score
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceMatch {
    pub name: String,
    pub category: Option<String>,
    pub era: Option<String>,
    pub style: Option<String>,
    pub description: String,
    pub similarity: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(description: &str, embedding_text: Option<&str>) -> ClothingReference {
        ClothingReference {
            id: "id".into(),
            name: "name".into(),
            category: None,
            subcategory: None,
            era: None,
            style: None,
            description: description.into(),
            embedding_text: embedding_text.map(str::to_string),
            embedding: None,
            review_status: "draft".into(),
            source_note: None,
            version: 1,
            created_at: NaiveDateTime::default(),
            updated_at: NaiveDateTime::default(),
        }
    }

    #[test]
    fn a_missing_search_text_falls_back_to_the_description() {
        let r = reference("긴 설명", None);
        assert_eq!(r.text_for_embedding(), "긴 설명");
    }

    #[test]
    fn a_search_text_wins_over_the_description() {
        let r = reference("긴 설명", Some("짧은 검색 문장"));
        assert_eq!(r.text_for_embedding(), "짧은 검색 문장");
    }

    /// 빈 문자열을 그대로 임베딩하면 모든 질의에 같은 유사도로 걸린다.
    #[test]
    fn a_blank_search_text_is_ignored() {
        assert_eq!(
            reference("긴 설명", Some("")).text_for_embedding(),
            "긴 설명"
        );
        assert_eq!(
            reference("긴 설명", Some("   ")).text_for_embedding(),
            "긴 설명"
        );
    }
}
