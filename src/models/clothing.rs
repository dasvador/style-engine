use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::models::style_vocab::{
    Role, Saturation, Silhouette, Style, StyleGenre, Thickness, Tone, Weight,
};

/// DB row for clothing table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Clothing {
    pub id: String,
    pub name: String,
    pub category: String,
    pub gender: Option<String>,
    /// 사용자가 고르는 스타일 장르. 표준 밖의 값이 들어 있으면 행 디코딩이 실패한다.
    pub style_mood: Option<StyleGenre>,
    pub color: Option<String>,
    pub thickness: Thickness,
    pub image_url: Option<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub visual_weight: Option<String>,
    pub texture_depth: Option<String>,
    pub visual_weight_v2: Option<i8>,
    pub texture_depth_v2: Option<i8>,
    pub grounding_score: Option<i8>,
    pub shadow_tone: Option<String>,
    pub silhouette_volume: Option<Silhouette>,
    pub material_primary: Option<String>,
    pub sub_category: Option<String>,
    pub floating_score: Option<i8>,
    pub strong_style_score: Option<i8>,
    pub texture_keywords: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Request body for creating clothing
#[derive(Debug, Deserialize, Validate)]
pub struct CreateClothingRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1, max = 50))]
    pub category: String,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub image_url: Option<String>,
    pub seasons: Option<Vec<String>>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
}

/// Request body for updating clothing
#[derive(Debug, Deserialize)]
pub struct UpdateClothingRequest {
    pub name: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub image_url: Option<String>,
    pub seasons: Option<Vec<String>>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
}

/// Response from OpenAI Vision API clothing analysis
#[derive(Debug, Deserialize)]
pub struct VisionAnalysisResult {
    pub is_clothing: bool,
    pub name: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub thickness: Option<Thickness>,
    pub seasons: Option<Vec<String>>,
    pub rejection_reason: Option<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Option<Vec<String>>,
    /// 이 옷이 주로 어느 쪽 옷장에 들어가는지. 모르면 `None`.
    pub gender: Option<String>,
    /// 어울리는 장르 전부. 한 벌이 여러 장르에 들어갈 수 있다 —
    /// 옥스퍼드 셔츠는 클래식이면서 아메카지이고 프레피다.
    #[serde(default)]
    pub style_genres: Vec<String>,
}

/// Request body for image-based clothing upload
#[derive(Debug, Deserialize)]
pub struct ImageUploadRequest {
    pub image_data: String,
}

/// Pass 1 result from Vision API: simple description of the clothing item
#[derive(Debug, Deserialize)]
pub struct Pass1Result {
    pub description: String,
    /// 관찰된 의류 종류. 레퍼런스 검색을 같은 종류로 좁히는 데만 쓴다 — 최종
    /// 카테고리는 Pass 2 가 정한다.
    ///
    /// `Option` 인 이유: 이 필드는 나중에 추가됐고, 모델이 빠뜨리거나 어휘 밖의
    /// 값을 돌려줄 수 있다. 그때는 `None` 이 되어 선필터 없이 전체를 검색한다 —
    /// 추가 전과 같은 동작이다.
    #[serde(default)]
    pub category: Option<String>,
}

impl Pass1Result {
    /// 검색 선필터로 쓸 카테고리. 어휘 밖의 값은 버린다.
    ///
    /// 모르는 값을 그대로 넘기면 어떤 레퍼런스와도 일치하지 않아 후보가 0개가 되고,
    /// 호출자는 그것을 "레퍼런스 없음" 으로 읽는다. 검색을 안 한 것과 레퍼런스가
    /// 없는 것은 다르므로, 모르는 값은 선필터를 끄는 쪽으로 처리한다.
    pub fn search_category(&self) -> Option<String> {
        const KNOWN: [&str; 8] = [
            "상의",
            "하의",
            "아우터",
            "신발",
            "가방",
            "액세서리",
            "모자",
            "벨트",
        ];
        let c = self.category.as_deref()?.trim();
        KNOWN.contains(&c).then(|| c.to_string())
    }
}

/// Response DTO with seasons included
#[derive(Debug, Serialize)]
pub struct ClothingResponse {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: Option<String>,
    pub thickness: Thickness,
    pub image_url: Option<String>,
    pub seasons: Vec<String>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub style: Option<Style>,
    pub weight: Option<Weight>,
    pub role: Option<Role>,
    pub color_temperature: Option<String>,
    pub versatility: Option<String>,
    pub statement_level: Option<i8>,
    pub formality_level: Option<i8>,
    pub texture_worlds: Vec<String>,
    /// 이 옷에 붙은 장르 전부. 등록 직후 화면이 확인·수정할 수 있게 함께 내려 준다.
    #[serde(default)]
    pub style_genres: Vec<String>,
    pub gender: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[cfg(test)]
mod pass1_tests {
    use super::Pass1Result;

    fn pass1(category: Option<&str>) -> Pass1Result {
        Pass1Result {
            description: "서술".into(),
            category: category.map(str::to_string),
        }
    }

    #[test]
    fn a_known_category_narrows_the_search() {
        assert_eq!(
            pass1(Some("아우터")).search_category().as_deref(),
            Some("아우터")
        );
        assert_eq!(
            pass1(Some(" 가방 ")).search_category().as_deref(),
            Some("가방")
        );
    }

    /// 어휘 밖의 값으로 좁히면 후보가 0개가 되고, 호출자는 그것을 "레퍼런스 없음"
    /// 으로 읽는다. 검색을 안 한 것과 레퍼런스가 없는 것은 다르다.
    #[test]
    fn an_unknown_category_does_not_narrow_anything() {
        assert_eq!(pass1(Some("outerwear")).search_category(), None);
        assert_eq!(pass1(Some("원피스")).search_category(), None);
        assert_eq!(pass1(Some("")).search_category(), None);
        assert_eq!(pass1(None).search_category(), None);
    }
}
