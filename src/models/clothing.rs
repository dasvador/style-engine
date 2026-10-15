use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer, Serialize};
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

/// Vision 응답의 어휘 필드를 너그럽게 읽는다. 어휘 밖의 값은 비우고 로그를 남긴다.
///
/// 어휘 타입(`Style`, `Tone` …)은 원래 엄격하다 — 표준 밖의 값이 DB 에 섞이면 어느
/// 규칙에도 걸리지 않으면서 조용히 살아남기 때문이다. 그 엄격함을 LLM 응답에 그대로
/// 적용하면 필드 하나 때문에 응답 전체가 거부된다. 2026-10-11 여성복 실사진 25장 평가에서
/// 모델이 스타일을 `모던` 으로 답해 4장이 재시도 끝에 등록 자체가 실패했다.
///
/// 그래서 경계에서만 푼다. 모르는 값은 `None` 이 되어 DB 에는 표준 값만 들어가고,
/// 나머지 분석은 살아서 등록된다.
fn lenient_vocab<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr,
{
    let raw: Option<serde_json::Value> = Option::deserialize(d)?;
    let text = match raw {
        Some(serde_json::Value::String(s)) => s,
        Some(serde_json::Value::Null) | None => return Ok(None),
        Some(other) => other.to_string(),
    };
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<T>() {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            tracing::warn!(
                field = std::any::type_name::<T>(),
                value = text,
                "Vision 이 어휘 밖의 값을 돌려줘 비워 둔다"
            );
            Ok(None)
        }
    }
}

/// Response from OpenAI Vision API clothing analysis
#[derive(Debug, Deserialize)]
pub struct VisionAnalysisResult {
    pub is_clothing: bool,
    pub name: Option<String>,
    pub category: Option<String>,
    pub color: Option<String>,
    #[serde(default, deserialize_with = "lenient_vocab")]
    pub thickness: Option<Thickness>,
    pub seasons: Option<Vec<String>>,
    pub rejection_reason: Option<String>,
    #[serde(default, deserialize_with = "lenient_vocab")]
    pub tone: Option<Tone>,
    #[serde(default, deserialize_with = "lenient_vocab")]
    pub saturation: Option<Saturation>,
    #[serde(default, deserialize_with = "lenient_vocab")]
    pub style: Option<Style>,
    #[serde(default, deserialize_with = "lenient_vocab")]
    pub weight: Option<Weight>,
    #[serde(default, deserialize_with = "lenient_vocab")]
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

impl Clothing {
    /// 이미지 프롬프트에 넘길 소재·구조 설명.
    ///
    /// `material_primary` 는 "cotton" 처럼 소재만, `texture_keywords` 는
    /// "cotton jersey, relaxed crewneck" 처럼 짜임과 구조까지 담는다. 둘 중 하나만
    /// 고르면 아깝다 — 워크 셔츠처럼 texture 가 "faded" 뿐인 경우도 있고, 백팩처럼
    /// texture 가 소재를 이미 포함하는 경우도 있다. 그래서 합치되, 소재 이름이 이미
    /// texture 안에 있으면 중복해서 넣지 않는다.
    ///
    /// 이미지 모델에게 아이템 이름만 주면 "가방" 수준으로 뭉개진다. 소재와 구조를
    /// 같이 줘야 가죽 결과 재봉선이 살아난다.
    ///
    /// 홈 추천과 상담이 같은 것을 쓴다. 상담만 `material_primary` 하나("cotton")를 넣었을
    /// 때는 상담 사진만 핏과 소재감이 덜 살았다.
    pub fn image_material(&self) -> Option<String> {
        let tex = self
            .texture_keywords
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let mat = self
            .material_primary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match (mat, tex) {
            (Some(m), Some(t)) if t.to_lowercase().contains(&m.to_lowercase()) => {
                Some(t.to_string())
            }
            (Some(m), Some(t)) => Some(format!("{m}, {t}")),
            (Some(m), None) => Some(m.to_string()),
            (None, Some(t)) => Some(t.to_string()),
            (None, None) => None,
        }
    }
}

impl VisionAnalysisResult {
    /// 옷장에 넣을 아이템인지. 모델의 `is_clothing` 을 그대로 믿지 않는 경우가 하나 있다.
    ///
    /// 가방 사진에 모델이 `is_clothing: false` 를 주면서 카테고리 `가방` 과 나머지 속성을
    /// 전부 채우고 거절 사유는 비운 적이 있다(2026-10-11, 3회 중 3회). "가방은 옷이
    /// 아니다" 로 읽은 자기모순이다. 거절 사유 없이 알려진 카테고리를 채웠다면 아이템으로
    /// 받아들인다. 거절 사유가 있으면 모델의 판단을 따른다 — 그때는 정말로 옷이 아닌
    /// 사진일 가능성이 높다.
    pub fn is_wardrobe_item(&self) -> bool {
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
        if self.is_clothing {
            return true;
        }
        let no_reason = self
            .rejection_reason
            .as_deref()
            .is_none_or(|r| r.trim().is_empty());
        let known_category = self
            .category
            .as_deref()
            .is_some_and(|c| KNOWN.contains(&c.trim()));
        no_reason && known_category
    }

    /// 저장할 이름. 비어 있으면 색상과 카테고리로 만든 임시 이름을 쓴다.
    ///
    /// 프롬프트가 이름을 비우지 말라고 해도, 신발·가방 사진에서 빈 문자열이 돌아왔다
    /// (2026-10-11 평가 25장 중 3장). `Option` 검사만으로는 빈 문자열이 통과해서 이름
    /// 없는 옷이 저장됐다. 등록을 실패시키기보다 고칠 수 있는 이름으로 남긴다 — 나머지
    /// 분석은 멀쩡하고, 이름은 상세 화면에서 바꿀 수 있다.
    pub fn name_or_fallback(&self) -> Option<String> {
        if let Some(n) = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            return Some(n.to_string());
        }
        let category = self
            .category
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())?;
        Some(
            match self
                .color
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            {
                Some(color) => format!("{color} {category}"),
                None => category.to_string(),
            },
        )
    }
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

#[cfg(test)]
mod vision_result_tests {
    use super::*;

    fn parse(json: &str) -> VisionAnalysisResult {
        serde_json::from_str(json).expect("파싱 실패")
    }

    /// 어휘 밖의 스타일 하나 때문에 응답 전체가 거부되면 등록이 실패한다.
    #[test]
    fn an_unknown_style_is_dropped_instead_of_failing() {
        let r = parse(
            r#"{"is_clothing": true, "name": "블랙 드레이프 스커트", "style": "모던", "tone": "어두움"}"#,
        );
        assert_eq!(r.style, None);
        assert_eq!(r.tone, Some(Tone::Dark));
    }

    #[test]
    fn known_vocabulary_still_parses() {
        let r = parse(
            r#"{"is_clothing": true, "style": "포멀", "thickness": "thick", "role": "베이스"}"#,
        );
        assert_eq!(r.style, Some(Style::Formal));
        assert_eq!(r.thickness, Some(Thickness::Thick));
        assert_eq!(r.role, Some(Role::Base));
    }

    #[test]
    fn missing_null_and_blank_vocabulary_are_none() {
        let r = parse(r#"{"is_clothing": true, "style": null, "tone": ""}"#);
        assert_eq!(r.style, None);
        assert_eq!(r.tone, None);
        assert_eq!(r.weight, None);
    }

    #[test]
    fn a_blank_name_falls_back_to_colour_and_category() {
        let r =
            parse(r#"{"is_clothing": true, "name": "  ", "category": "신발", "color": "베이지"}"#);
        assert_eq!(r.name_or_fallback().as_deref(), Some("베이지 신발"));
        let r = parse(r#"{"is_clothing": true, "name": "", "category": "가방"}"#);
        assert_eq!(r.name_or_fallback().as_deref(), Some("가방"));
    }

    /// 가방을 "옷이 아니다" 로 읽은 자기모순 응답.
    #[test]
    fn a_bag_marked_not_clothing_without_a_reason_is_still_an_item() {
        let r = parse(
            r#"{"is_clothing": false, "name": null, "category": "가방", "color": "스트라이프", "rejection_reason": null}"#,
        );
        assert!(r.is_wardrobe_item());
        assert_eq!(r.name_or_fallback().as_deref(), Some("스트라이프 가방"));
    }

    #[test]
    fn a_real_rejection_is_respected() {
        let r = parse(
            r#"{"is_clothing": false, "category": "가방", "rejection_reason": "음식 사진입니다"}"#,
        );
        assert!(!r.is_wardrobe_item());
        let r = parse(r#"{"is_clothing": false, "category": null, "rejection_reason": null}"#);
        assert!(!r.is_wardrobe_item());
    }

    #[test]
    fn a_real_name_is_kept() {
        let r = parse(r#"{"is_clothing": true, "name": " 그레이 롱 코트 ", "category": "아우터"}"#);
        assert_eq!(r.name_or_fallback().as_deref(), Some("그레이 롱 코트"));
    }

    #[test]
    fn no_name_and_no_category_gives_nothing() {
        let r = parse(r#"{"is_clothing": true, "name": ""}"#);
        assert_eq!(r.name_or_fallback(), None);
    }
}
