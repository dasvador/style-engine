use serde::{Deserialize, Serialize};

/// Request body for recommendation endpoint
#[derive(Debug, Deserialize)]
pub struct RecommendationRequest {
    pub occasion: Option<String>,
    pub style_preference: Option<String>,
    pub gender: Option<String>,
    pub style_mood: Option<String>,
    /// 이미 보여 준 코디들(아이템 id 목록). 다른 무드에서 본 것도 포함한다 — 같은 코디가
    /// 무드만 바꿔 다시 나오지 않게 한다. 멀티 추천만 읽는다.
    #[serde(default)]
    pub avoid_outfits: Vec<Vec<String>>,
    /// 개발용 기온 덮어쓰기. 서버 환경변수 `ALLOW_DEBUG_WEATHER=1` 일 때만 쓰인다.
    #[serde(default)]
    pub debug_temperature: Option<f64>,
}

/// Response DTO for recommendation (to client)
#[derive(Debug, Serialize)]
pub struct RecommendationResponse {
    pub recommendation: String,
    pub outfit: Vec<OutfitItem>,
    pub weather_summary: String,
    pub tips: Vec<String>,
}

/// Outfit item returned to client — includes image_url
#[derive(Debug, Serialize)]
pub struct OutfitItem {
    /// 옷장 아이템 id. 화면이 이미 본 코디를 다음 요청의 `avoid_outfits` 로 돌려보낼 때
    /// 쓴다. 이름으로 비교하면 같은 이름의 옷이 둘일 때 틀린다.
    pub id: String,
    pub category: String,
    pub name: String,
    pub reason: String,
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// 이 옷을 어떻게 입는지(영문, 이미지 프롬프트에 그대로 들어간다). 추운 날 코트는
    /// 여며 입은 상태로 보온을 판정하므로(`serving_ranker::outer_wear`) 같은 상태를 넘긴다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wear: Option<String>,
}

/// Outfit item from OpenAI response (no image_url)
///
/// `id` 가 권위 있는 선택값이다 — 후보 목록에 서버가 넣어 보여 준 의류 id 를 그대로 돌려받는다
/// (`services::item_resolution`). `name` 은 참고용이고, 화면에는 옷장의 이름을 쓴다. 모델이
/// `clothing_id` 로 적어도 받는다. 빠져 있으면 `None` — 신규 응답에서는 거절된다.
#[derive(Debug, Clone, Deserialize)]
pub struct AiOutfitItem {
    pub category: String,
    #[serde(default, alias = "clothing_id")]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

/// Shape we expect from OpenAI JSON response (single candidate)
#[derive(Debug, Clone, Deserialize)]
pub struct AiRecommendation {
    pub recommendation: String,
    pub outfit: Vec<AiOutfitItem>,
    pub weather_summary: String,
    pub tips: Vec<String>,
    /// 글램 후보에서 시선이 모이는 옷의 슬롯(상의·하의·아우터). 다른 무드는 비어 있다.
    #[serde(default)]
    pub focus: Option<String>,
}

/// Multi-candidate response from OpenAI
#[derive(Debug, Deserialize)]
pub struct AiMultiRecommendation {
    pub candidates: Vec<AiRecommendation>,
}

/// 후보 코디 (style_engine 점수 + 리센시 페널티 + 다양성 보너스)
#[derive(Debug, Clone)]
pub struct OutfitCandidate {
    pub top_id: Option<String>,
    pub bottom_id: Option<String>,
    pub outer_id: Option<String>,
    pub shoes_id: Option<String>,
    pub bag_id: Option<String>,

    pub style_score: i32,
    /// `style_score` 에 들어 있는 피드백 보정분. 엔진 점수와 따로 보려고 떼어 둔다.
    pub feedback_bonus: i32,
    pub recency_penalty: i32,
    pub diversity_bonus: i32,
    /// 오늘 날씨에 경계인 착장의 감점. 실패는 감점이 아니라 후보에서 뺀다.
    pub weather_penalty: i32,
    pub final_score: i32,

    /// 원본 AI 추천 인덱스 (candidates 배열 내 위치)
    pub ai_candidate_index: usize,
}

// ─── 3-모드 추천 시스템 ───

#[derive(Debug, Clone, Serialize)]
pub struct ScoringDetail {
    pub style_score: i32,
    pub recency_penalty: i32,
    pub diversity_bonus: i32,
    pub dormant_bonus: i32,
    /// 오늘 날씨에 경계인 착장의 감점(`serving_ranker::compute_today_fit` 가 Borderline).
    /// 착장 평가·취향·반복과 따로 남긴다.
    pub weather_penalty: i32,
    pub final_score: i32,
}

#[derive(Debug, Serialize)]
pub struct ModeRecommendation {
    pub mode: String,
    pub mode_label: String,
    pub mode_description: String,
    pub outfit: Vec<OutfitItem>,
    pub recommendation: String,
    pub weather_summary: String,
    pub tips: Vec<String>,
    pub score: i32,
    pub verdict: String,
    pub reason: String,
    pub revival_items: Vec<String>,
    pub scoring_detail: ScoringDetail,
    /// shadow 엔진(style_engine_v2 스타일 점수 + 상황 보정)의 점수. 응답 순서에는 쓰지
    /// 않는다 — 화면이 룩을 저장할 때 같이 돌려보내 반응 쌍과 비교한다.
    pub shadow_score: Option<i32>,
    /// 실루엣 shadow 가 더했을 점수.
    pub silhouette_delta: Option<i32>,
    /// 무드 shadow(`services::mood_shadow`)가 되돌렸을 감점의 합.
    pub mood_waiver: Option<i32>,
    /// `scoring_detail.style_score` 중 피드백(좋아요·아쉬워요) 보정분.
    pub feedback_bonus: i32,
}

#[derive(Debug, Serialize)]
pub struct MultiModeRecommendationResponse {
    pub modes: Vec<ModeRecommendation>,
    pub weather_summary: String,
    /// 후보가 모자랐을 때 화면에 알릴 한 줄. 날씨에 맞는 후보가 없거나, 무드를 읽을 수
    /// 있는 후보가 없어 기본 코디가 섞였을 때다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// 서버에 남긴 이 응답의 id(`recommendation_batch`). 화면은 두 코디 비교를 열 때 이 값만
    /// 보낸다 — 비교할 코디는 서버가 여기서 꺼낸다. 기록에 실패하면 비어 있다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
}

impl OutfitCandidate {
    pub fn new(
        top_id: Option<String>,
        bottom_id: Option<String>,
        outer_id: Option<String>,
        shoes_id: Option<String>,
        bag_id: Option<String>,
        ai_candidate_index: usize,
    ) -> Self {
        Self {
            top_id,
            bottom_id,
            outer_id,
            shoes_id,
            bag_id,
            style_score: 0,
            feedback_bonus: 0,
            recency_penalty: 0,
            diversity_bonus: 0,
            weather_penalty: 0,
            final_score: 0,
            ai_candidate_index,
        }
    }
}
