//! 스타일 도메인의 LLM task 정의.
//!
//! 이 모듈은 프롬프트를 만들고 결과를 도메인 타입으로 되돌리는 일만 한다.
//! 어떤 provider의 어떤 모델이 호출되는지, 재시도·타임아웃·비용 계측을 어떻게 하는지는
//! [`crate::services::llm`]가 책임진다. 여기서 모델명을 언급해서는 안 된다.

use std::sync::Arc;

use crate::models::clothing::{Pass1Result, VisionAnalysisResult};
use crate::models::recommendation::{AiMultiRecommendation, AiRecommendation};
use crate::models::reference::ReferenceMatch;
use crate::models::weather::CurrentWeather;
use crate::services::embedding::{EmbeddingService, SearchScope};
use crate::services::llm::{ChatRequest, LlmClient, LlmTask, Message};

// ─── 1) get_outfit_recommendation ───

pub async fn get_outfit_recommendation(
    llm: &LlmClient,
    weather: &CurrentWeather,
    clothes: &[String],
    occasion: Option<&str>,
    style_preference: Option<&str>,
) -> anyhow::Result<AiRecommendation> {
    let clothes_list = if clothes.is_empty() {
        "사용자가 등록한 옷이 없습니다. 일반적인 추천을 해주세요.".to_string()
    } else {
        clothes.join("\n")
    };

    let occasion_text = occasion.unwrap_or("일상");
    let style_text = style_preference.unwrap_or("편한 스타일");

    let system_prompt = r#"당신은 다양한 현대 남녀 패션 스타일과 의류 구성에 익숙한 코디 보조 AI입니다.

중요 원칙:
- 최종 스타일 판단은 별도의 규칙 엔진이 담당합니다.
- 당신의 역할은 주어진 옷장 후보 안에서 날씨와 상황에 맞는 "후보 코디안"을 조합하는 것입니다.
- 규칙 엔진처럼 강하게 판정하거나 단정하지 마세요.
- 옷장에 없는 아이템을 절대 만들어내지 마세요.
- 반드시 입력으로 제공된 아이템명만 사용하세요.

추천 원칙:
1. 날씨와 상황을 우선 고려하세요.
2. 아우터는 날씨상 필요할 때만 포함하세요.
3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 가능하면 안정적인 역할(베이스/구조템/연결템) 아이템을 우선 선택하세요.
5. 이너는 가능하면 중립적이고 활용도 높은 아이템을 우선 선택하세요.
6. 과하게 비슷한 색/무드로 몰리는 조합은 피하세요.
7. 설명은 과장하지 말고, 왜 무난하고 안정적인 후보인지 간단히 설명하세요.

반드시 JSON 형식으로 응답하세요."#;

    let user_prompt = format!(
        r#"현재 조건:
- 기온: {temp}°C (체감 {feels}°C)
- 습도: {humidity}%
- 바람: {wind} km/h
- 날씨: {desc}
- 상황: {occasion}
- 선호 스타일: {style}

사용자 옷장 후보:
{clothes}

작업:
- 위 옷장 안에서만 코디 후보를 구성하세요.
- 날씨상 불필요하면 아우터를 억지로 넣지 마세요.
- 존재감 강한 아이템을 여러 개 겹치지 마세요.
- 추천은 "후보 제안" 성격으로 작성하세요.

응답 JSON 형식:
{{
  "recommendation": "전체 추천 요약 (2~3문장)",
  "outfit": [
    {{ "category": "상의", "name": "정확한 아이템명", "reason": "선택 이유" }},
    {{ "category": "하의", "name": "정확한 아이템명", "reason": "선택 이유" }},
    {{ "category": "아우터", "name": "정확한 아이템명", "reason": "선택 이유" }}
  ],
  "weather_summary": "날씨 요약 한 줄",
  "tips": ["실용적인 팁 1", "실용적인 팁 2"]
}}

주의:
- 아우터가 필요 없으면 outfit 배열에서 생략 가능
- 절대 옷장에 없는 이름을 쓰지 마세요"#,
        temp = weather.temperature,
        feels = weather.apparent_temperature,
        humidity = weather.humidity,
        wind = weather.wind_speed,
        desc = weather.weather_description,
        clothes = clothes_list,
        occasion = occasion_text,
        style = style_text,
    );

    Ok(llm
        .chat_json::<AiRecommendation>(
            LlmTask::OutfitRecommendation,
            ChatRequest::new(vec![Message::user_text(user_prompt)])
                .system(system_prompt)
                .json(),
        )
        .await?)
}

// ─── 1b) get_outfit_candidates (3 candidates for diversity scoring) ───

/// 카테고리별 그룹화된 옷장 데이터. flat list 대신 슬롯별 후보를 분리해 LLM 혼동 방지.
pub struct GroupedClothes {
    pub tops: Vec<String>,
    pub bottoms: Vec<String>,
    pub outers: Vec<String>,
    pub shoes: Vec<String>,
    pub bags: Vec<String>,
}

/// 슬롯별 후보 목록에서 5가지 코디 초안을 받는다.
///
/// 아우터 규칙을 온도로 못박고 응답 예시에도 아우터를 넣어 둔 이유: 예시에 아우터 줄이
/// 없고 "필요할 때만" 이라고만 적혀 있던 동안, 모델은 15°C 에서도 5개 후보 전부를
/// 아우터 없이 채웠다. 그러면 블레이저·코트 같은 아우터는 shortlist 까지 올라와도
/// 후보에 한 번도 등장하지 못해 규칙 엔진이 비교할 기회조차 없다.
pub async fn get_outfit_candidates(
    llm: &LlmClient,
    weather: &CurrentWeather,
    clothes: &GroupedClothes,
    occasion: Option<&str>,
    style_preference: Option<&str>,
    recent_items_hint: &str,
) -> anyhow::Result<AiMultiRecommendation> {
    let clothes_grouped = format!(
        "[상의 후보]\n{tops}\n\n[하의 후보]\n{bottoms}\n\n[아우터 후보]\n{outers}\n\n[신발 후보]\n{shoes}\n\n[가방 후보]\n{bags}",
        tops = if clothes.tops.is_empty() {
            "(없음)".to_string()
        } else {
            clothes.tops.join("\n")
        },
        bottoms = if clothes.bottoms.is_empty() {
            "(없음)".to_string()
        } else {
            clothes.bottoms.join("\n")
        },
        outers = if clothes.outers.is_empty() {
            "(없음)".to_string()
        } else {
            clothes.outers.join("\n")
        },
        shoes = if clothes.shoes.is_empty() {
            "(없음)".to_string()
        } else {
            clothes.shoes.join("\n")
        },
        bags = if clothes.bags.is_empty() {
            "(없음)".to_string()
        } else {
            clothes.bags.join("\n")
        },
    );

    let occasion_text = occasion.unwrap_or("일상");
    let style_text = style_preference.unwrap_or("편한 스타일");

    let today = chrono::Local::now().format("%Y-%m-%d (%A)").to_string();

    let system_prompt = r#"당신은 다양한 현대 남녀 패션 스타일과 의류 구성에 익숙한 코디 보조 AI입니다.

역할:
- 주어진 슬롯별 후보 목록에서만 아이템을 골라 코디를 구성하는 "초안 생성기"입니다.
- 최종 판단은 별도 규칙 엔진이 합니다. 강하게 단정하지 마세요.

슬롯 규칙 (절대 준수):
- 상의 슬롯에는 [상의 후보]에서만 선택하라.
- 하의 슬롯에는 [하의 후보]에서만 선택하라.
- 아우터 슬롯에는 [아우터 후보]에서만 선택하라.
- 신발 슬롯에는 [신발 후보]에서만 선택하라.
- 가방 슬롯에는 [가방 후보]에서만 선택하라.
- 다른 슬롯의 아이템을 가져오지 마라.
- 후보 목록에 없는 이름을 만들지 마라.
- 이름은 후보 목록의 문자열을 그대로 복사하라.
- 확신이 없으면 해당 슬롯을 빈 문자열로 두어라. 서버가 후처리로 채운다.

추천 원칙:
1. 날씨와 상황을 우선 고려하세요.
2. 아우터는 기온으로 판단하세요. 취향이 아니라 온도가 기준입니다.
   - 20°C 이상: 아우터를 넣지 마세요.
   - 15~20°C: 이너가 얇으면 넣고, 이너가 두꺼우면 생략해도 됩니다.
   - 15°C 미만: 기본적으로 넣으세요. 생략하려면 이너 자체가 그만큼 두꺼워야 합니다.
   아우터를 넣는 후보는 이너를 얇게 골라도 됩니다 — 보온은 조합 전체로 맞추면 됩니다.
3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너는 중립적이고 활용도 높은 아이템 우선.
6. 같은 색/무드로 몰리지 마세요.

5가지 서로 다른 후보 코디를 제안하세요.
반드시 JSON 형식으로 응답하세요."#;

    let user_prompt = format!(
        r#"오늘 날짜: {today}

현재 조건:
- 기온: {temp}°C (체감 {feels}°C)
- 습도: {humidity}%
- 바람: {wind} km/h
- 날씨: {desc}
- 상황: {occasion}
- 선호 스타일: {style}
{recent_section}
{clothes}

작업:
- 위 슬롯별 후보 목록에서만 5가지 서로 다른 코디 후보를 구성하세요.
- 각 후보는 다른 아이템 조합이어야 합니다.
- 상의와 하의 중 최소 하나는 후보마다 달라야 합니다.
- 기온이 20°C 미만이면 아우터를 넣은 후보와 넣지 않은 후보를 **둘 다** 포함하세요.
  어느 쪽이 나은지는 규칙 엔진이 고릅니다. 한쪽으로만 5개를 채우지 마세요.
- 신발은 반드시 [신발 후보]에서만 고르세요.
- 가방은 반드시 [가방 후보]에서만 고르세요.

응답 JSON 형식:
{{
  "candidates": [
    {{
      "recommendation": "후보 1 요약 (1~2문장)",
      "outfit": [
        {{ "category": "상의", "name": "정확한 아이템명", "reason": "선택 이유" }},
        {{ "category": "하의", "name": "정확한 아이템명", "reason": "선택 이유" }},
        {{ "category": "아우터", "name": "정확한 아이템명", "reason": "선택 이유" }},
        {{ "category": "신발", "name": "정확한 아이템명", "reason": "선택 이유" }},
        {{ "category": "가방", "name": "정확한 아이템명", "reason": "선택 이유" }}
      ],
      "weather_summary": "날씨 요약 한 줄",
      "tips": ["팁"]
    }},
    {{
      "recommendation": "후보 2 요약",
      "outfit": [...],
      "weather_summary": "...",
      "tips": [...]
    }},
    {{
      "recommendation": "후보 3 요약",
      "outfit": [...],
      "weather_summary": "...",
      "tips": [...]
    }},
    {{ "recommendation": "후보 4 요약", "outfit": [...], "weather_summary": "...", "tips": [...] }},
    {{ "recommendation": "후보 5 요약", "outfit": [...], "weather_summary": "...", "tips": [...] }}
  ]
}}

주의:
- 기온이 높아 아우터가 불필요한 후보만 아우터를 생략하세요. 위 예시는 아우터를 넣은 모양입니다.
- 절대 옷장에 없는 이름을 쓰지 마세요"#,
        temp = weather.temperature,
        feels = weather.apparent_temperature,
        humidity = weather.humidity,
        wind = weather.wind_speed,
        desc = weather.weather_description,
        clothes = clothes_grouped,
        occasion = occasion_text,
        style = style_text,
        today = today,
        recent_section = if recent_items_hint.is_empty() {
            String::new()
        } else {
            format!(
                "\n최근 추천된 아이템 (가능하면 다른 조합을 시도하세요):\n{}\n",
                recent_items_hint
            )
        },
    );

    Ok(llm
        .chat_json::<AiMultiRecommendation>(
            LlmTask::OutfitCandidates,
            ChatRequest::new(vec![Message::user_text(user_prompt)])
                .system(system_prompt)
                .json(),
        )
        .await?)
}

// ─── 2) analyze_clothing_image (fallback, no RAG) ───

pub async fn analyze_clothing_image(
    llm: &LlmClient,
    image_data_url: &str,
) -> anyhow::Result<VisionAnalysisResult> {
    let system_prompt = r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 분석 AI입니다.
사용자가 업로드한 이미지에서 의류/신발/가방/모자/벨트 등 패션 아이템을 분석하여 구조화된 정보를 추출하세요.

가장 중요한 원칙:
1. 보이는 것만 바탕으로 판단하세요.
2. 브랜드나 모델명을 이미지에서 확실히 식별할 수 없는 경우 절대 추측하지 마세요.
3. 확실하지 않으면 일반화된 구체명으로 작성하세요.
4. role, versatility, statement_level, formality_level 등은 "일반적인 활용성 기준의 1차 추정치"로 판단하세요.
5. 거짓 정밀함(false precision)을 피하세요.

name 작성 원칙:
- 가장 우선은 "정확성"입니다.
- 확실히 보이면: "색상 + 브랜드/모델명 + 소재 + 아이템명"
- 확실하지 않으면: "색상 + 소재/스타일 + 구체적 아이템명"
- 단순히 "청바지", "운동화"처럼 너무 일반적인 이름은 피하세요.
- 하지만 확실하지 않은 브랜드/모델명을 억지로 넣는 것보다 일반화된 구체명이 더 낫습니다.

예시:
- 확실할 때: "그레이 New Balance 990v3 스웨이드 스니커"
- 불확실할 때: "그레이 러닝 스타일 스웨이드 스니커"
- 확실할 때: "올리브 백사틴 M-43 필드 자켓"
- 불확실할 때: "올리브 필드 자켓 스타일 아우터"

반드시 다음 JSON 형식으로 응답하세요:
{
  "is_clothing": true,
  "name": "구체적인 아이템 이름",
  "category": "카테고리",
  "color": "색상",
  "thickness": "thin/medium/thick 중 하나",
  "seasons": ["계절1", "계절2"],
  "tone": "밝음/중간/어두움 중 하나",
  "saturation": "낮음/중간/높음 중 하나",
  "style": "베이직/워크/밀리터리/포멀/스포츠 중 하나",
  "weight": "가벼움/중간/무거움 중 하나",
  "role": "베이스/포인트/약한포인트/연결템/구조템 중 하나",
  "color_temperature": "warm/cool/neutral 중 하나",
  "versatility": "universal/flexible/situational/statement 중 하나",
  "statement_level": 1~5 사이 정수,
  "formality_level": 1~5 사이 정수,
  "texture_worlds": ["해당하는 텍스처 월드 모두 선택"],
  "gender": "female/male/unisex 중 하나",
  "style_genres": ["어울리는 장르 모두"],
  "rejection_reason": null
}

추가 규칙:
- is_clothing이 true이면 name은 null이면 안 됩니다.
- gender 는 이 옷이 주로 어느 쪽 옷장에 들어갈지입니다. 어느 쪽이든 입을 수 있으면 unisex.
- style_genres 는 이 옷이 어울리는 장르를 **모두** 고릅니다. 한 벌이 여러 장르에
  들어갈 수 있습니다 — 옥스퍼드 셔츠는 classic 이면서 amekaji 이고 preppy 입니다.
  고를 수 있는 값: minimal(장식 없는 단순한 실루엣, 적은 색상),
  classic(셔츠·재킷·트렌치의 단정한 핏), romantic(부드러운 소재와 곡선),
  modern_chic(선명한 실루엣과 강한 대비), bohemian(자연 소재와 느슨한 레이어링),
  street(오버사이즈와 그래픽, 데님·카고), mannish(테일러링과 넓은 어깨),
  sporty_casual(기능성 소재와 운동복 요소), amekaji(아메리칸 캐주얼),
  preppy(아이비리그), workwear(작업복), outdoor_casual(기능성 아웃도어).
  확신이 서는 것만 고르고, 애매하면 비워 두세요 — 틀린 장르에 들어가면 그 장르의
  추천이 전부 어긋납니다.
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요.
- texture_worlds는 workwear, military, tailoring, sweat, outdoor, minimal 중 복수 선택 가능.
- category는 상의, 하의, 아우터, 신발, 액세서리, 가방, 모자, 벨트 중 하나입니다."#;

    Ok(llm
        .chat_json::<VisionAnalysisResult>(
            LlmTask::VisionAnalyze,
            ChatRequest::new(vec![Message::user_image(
                "이 이미지를 분석하여 의류 정보를 추출해주세요. 브랜드는 확실할 때만 포함하세요.",
                image_data_url,
            )])
            .system(system_prompt)
            .json(),
        )
        .await?)
}

// ─── 3) analyze_clothing_pass1 ───

pub async fn analyze_clothing_pass1(
    llm: &LlmClient,
    image_data_url: &str,
) -> anyhow::Result<Pass1Result> {
    let system_prompt = r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 감정사 AI입니다.
이미지에 보이는 아이템의 외관적 특징을 검색/비교 가능한 형태로 서술하세요.

중요 원칙:
- 길이보다 "식별 가능한 특징의 밀도"가 더 중요합니다.
- 포켓 구조, 여밈 방식, 칼라 형태, 소재 질감, 워싱, 실루엣, 디테일 같은 비교 가능한 단서를 빠뜨리지 마세요.
- 보이지 않는 정보는 추측하지 말고 "확인 불가"로 두세요.
- 브랜드/모델명은 이 단계에서 추측하지 마세요.

아이템 종류에 따라 해당하는 항목을 포함하세요:

[아우터/상의]
1. 칼라 형태
2. 여밈 방식
3. 포켓 수/종류
4. 소재 질감과 무게감
5. 기장감
6. 주요 디테일
7. 색상

[하의/데님]
1. 핏
2. 소재 종류
3. 두께감
4. 색상/워싱 정도
5. 주요 디테일
6. 포켓 구조

[신발/스니커]
1. 종류
2. 소재
3. 솔 형태
4. 색상
5. 모델 식별에 도움이 되는 특징

[스웻셔츠/니트]
1. 넥라인
2. 소재
3. 무게감
4. 리브/커프스
5. 색상

반드시 JSON 형식으로 응답하세요:
{"category": "상의/하의/아우터/신발/가방/액세서리/모자/벨트 중 하나", "description": "한국어로 작성한 상세 서술. 180~300자 내외 권장, 단 식별 가능한 특징을 우선"}

category 는 레퍼런스를 같은 종류끼리만 비교하기 위한 것입니다. 보이는 그대로
고르세요. 애매하면 가장 가까운 것을 고르고, 목록에 없으면 빈 문자열로 두세요."#;

    Ok(llm
        .chat_json::<Pass1Result>(
            LlmTask::VisionPass1,
            ChatRequest::new(vec![Message::user_image(
                "이 의류의 시각적 특징을 서술해주세요. 브랜드는 추측하지 말고, 다른 아이템과 구분할 수 있는 구조적 특징에 집중하세요.",
                image_data_url,
            )])
            .system(system_prompt)
            .json(),
        )
        .await?)
}

// ─── 4) analyze_clothing_pass2 ───

pub async fn analyze_clothing_pass2(
    llm: &LlmClient,
    image_data_url: &str,
    references: &[ReferenceMatch],
) -> anyhow::Result<VisionAnalysisResult> {
    let ref_context: String = references
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "참고자료 {} (유사도 {:.0}%): {} (시대: {}, 스타일: {})\n{}",
                i + 1,
                r.similarity * 100.0,
                r.name,
                r.era.as_deref().unwrap_or("N/A"),
                r.style.as_deref().unwrap_or("N/A"),
                r.description,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system_prompt = format!(
        r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 감정사 AI입니다.

## 후보 레퍼런스 (유사도 순)
{ref_context}

중요 원칙:
1. 이미지를 먼저 직접 관찰하세요.
2. 레퍼런스는 참고자료이지 정답이 아닙니다.
3. 레퍼런스와 이미지가 핵심 식별 특징(실루엣, 포켓 구조, 여밈 방식, 소재감)에서 충분히 일치할 때만 해당 모델명을 사용하세요.
4. 부분적으로만 유사하면 레퍼런스를 참고하되, 일반화된 구체명으로 작성하세요.
5. 브랜드/모델명을 확신할 수 없으면 절대 추측하지 마세요.

판별 절차:
1. 아이템 종류를 먼저 판단
2. 같은 종류 레퍼런스만 비교
3. 핵심 특징이 강하게 일치하면 모델명 사용
4. 그렇지 않으면 이미지 관찰 기반 일반화된 구체명 사용

name 작성 원칙:
- 가장 우선은 정확성
- 형식: "색상 + 소재/스타일 + 아이템명"
- 확실할 때만 브랜드/모델명 포함
- name은 절대 null 금지

예시:
- 강하게 일치: "올리브 백사틴 M-43 필드 자켓"
- 부분 유사: "올리브 필드 자켓 스타일 아우터"
- 강하게 일치: "그레이 New Balance 990v3 스웨이드 스니커"
- 부분 유사: "그레이 러닝 스타일 스웨이드 스니커"

반드시 다음 JSON 형식으로 응답하세요:
{{
  "is_clothing": true,
  "name": "색상 + 소재/스타일 + 아이템명",
  "category": "상의/하의/아우터/신발/액세서리/가방/모자/벨트 중 하나",
  "color": "색상",
  "thickness": "thin/medium/thick 중 하나",
  "seasons": ["계절"],
  "tone": "밝음/중간/어두움 중 하나",
  "saturation": "낮음/중간/높음 중 하나",
  "style": "베이직/워크/밀리터리/포멀/스포츠 중 하나",
  "weight": "가벼움/중간/무거움 중 하나",
  "role": "베이스/포인트/약한포인트/연결템/구조템 중 하나",
  "color_temperature": "warm/cool/neutral 중 하나",
  "versatility": "universal/flexible/situational/statement 중 하나",
  "statement_level": 1~5,
  "formality_level": 1~5,
  "texture_worlds": ["해당하는 텍스처 월드 모두"],
  "gender": "female/male/unisex 중 하나",
  "style_genres": ["어울리는 장르 모두"],
  "rejection_reason": null
}}

추가 규칙:
- role과 versatility는 일반적인 활용성 기준의 1차 추정치입니다.
- gender 는 이 옷이 주로 어느 쪽 옷장에 들어갈지입니다. 어느 쪽이든 입을 수 있으면 unisex.
- style_genres 는 이 옷이 어울리는 장르를 **모두** 고릅니다. 한 벌이 여러 장르에
  들어갈 수 있습니다 — 옥스퍼드 셔츠는 classic 이면서 amekaji 이고 preppy 입니다.
  고를 수 있는 값: minimal(장식 없는 단순한 실루엣, 적은 색상),
  classic(셔츠·재킷·트렌치의 단정한 핏), romantic(부드러운 소재와 곡선),
  modern_chic(선명한 실루엣과 강한 대비), bohemian(자연 소재와 느슨한 레이어링),
  street(오버사이즈와 그래픽, 데님·카고), mannish(테일러링과 넓은 어깨),
  sporty_casual(기능성 소재와 운동복 요소), amekaji(아메리칸 캐주얼),
  preppy(아이비리그), workwear(작업복), outdoor_casual(기능성 아웃도어).
  확신이 서는 것만 고르고, 애매하면 비워 두세요 — 틀린 장르에 들어가면 그 장르의
  추천이 전부 어긋납니다.
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요."#
    );

    Ok(llm
        .chat_json::<VisionAnalysisResult>(
            LlmTask::VisionPass2,
            ChatRequest::new(vec![Message::user_image(
                "이 이미지를 분석하여 의류 정보를 추출해주세요. 위 레퍼런스는 참고하되, 핵심 특징이 충분히 일치할 때만 특정 모델명을 사용하세요.",
                image_data_url,
            )])
            .system(system_prompt)
            .json(),
        )
        .await?)
}

/// 이 값보다 1위 유사도가 낮으면 레퍼런스 없이 일반 분석으로 돌아간다.
///
/// `tests/retrieval_eval.rs` 가 같은 값으로 "정답인데 폴백" / "정답 없는데 통과" 를
/// 센다. 값을 바꿀 때는 그 스코어카드로 근거를 남길 것.
pub const RAG_MIN_SIMILARITY: f32 = 0.5;

/// Full 2-pass RAG pipeline: pass1 → embed → retrieve → pass2
pub async fn analyze_clothing_image_with_rag(
    llm: &LlmClient,
    image_data_url: &str,
    embedding_service: &Arc<EmbeddingService>,
) -> anyhow::Result<VisionAnalysisResult> {
    tracing::info!("RAG Pass 1: Getting image description...");
    let pass1 = analyze_clothing_pass1(llm, image_data_url).await?;
    tracing::info!("RAG Pass 1 result: {}", &pass1.description);

    // 같은 종류의 레퍼런스만 본다. 2026-10-11 측정에서 오답 6건이 전부 다른
    // 카테고리에 걸렸다 — 미디 스커트와 스트랩 힐이 `셀비지 데님 진` 설명을
    // 참고자료로 받고 있었다.
    let scope = SearchScope {
        category: pass1.search_category(),
        include_drafts: false,
    };
    let references = embedding_service
        .search_scoped(&pass1.description, 5, &scope)
        .await?;

    let top_similarity = references.first().map(|r| r.similarity).unwrap_or(0.0);
    let ref_names: Vec<&str> = references.iter().map(|r| r.name.as_str()).collect();
    tracing::info!(
        "RAG retrieved {} references (category={:?}, top sim={:.3}): {:?}",
        references.len(),
        scope.category,
        top_similarity,
        ref_names
    );

    // 그 종류의 레퍼런스가 아예 없으면 유사도를 볼 필요가 없다. 임계값은 분포가
    // 겹치지만 빈 후보는 겹치지 않는다 — 가장 확실한 폴백 신호다.
    if references.is_empty() {
        tracing::info!(
            "RAG has no reference for category {:?}, falling back to general analysis",
            scope.category
        );
        return analyze_clothing_image(llm, image_data_url).await;
    }

    if top_similarity < RAG_MIN_SIMILARITY {
        tracing::info!(
            "RAG similarity too low ({:.3}), falling back to general analysis",
            top_similarity
        );
        return analyze_clothing_image(llm, image_data_url).await;
    }

    tracing::info!("RAG Pass 2: Detailed analysis with reference context...");
    let result = analyze_clothing_pass2(llm, image_data_url, &references).await?;

    Ok(result)
}

// ─── 5) generate_outfit_explanation — 시그니처 변경: score 제거, verdict_label/strengths 분리 ───

pub async fn generate_outfit_explanation(
    llm: &LlmClient,
    items_desc: &str,
    verdict_label: &str,
    strengths_desc: &str,
    problems_desc: &str,
    suggestions_desc: &str,
) -> anyhow::Result<String> {
    let system_prompt = r#"당신은 다양한 현대 남녀 패션 스타일에 익숙한 스타일 코치입니다.
이미 결정된 평가 결과를 사용해 자연스럽고 친근한 한국어 설명문을 작성하세요.

중요 원칙:
- 당신은 코디를 새로 판단하지 않습니다.
- 입력으로 주어진 강점, 문제점, 개선 제안을 자연스럽게 풀어 설명만 합니다.
- 규칙 엔진의 결론을 바꾸거나 새로운 문제를 만들어내지 마세요.
- 2~4문장으로 간결하게 작성하세요.
- 좋은 점을 먼저, 아쉬운 점과 개선안을 뒤에 배치하세요.
- 구체적인 아이템명을 언급하세요.
- "베이스/포인트" 같은 역할 용어는 자연스러울 때만 사용하세요.
- 점수 숫자는 언급하지 마세요."#;

    let user_prompt = format!(
        "코디 구성:\n{items_desc}\n\n판정: {verdict}\n\n강점:\n{strengths}\n\n문제점:\n{problems}\n\n개선 제안:\n{suggestions}\n\n위 내용을 바탕으로 자연스럽고 짧은 한국어 설명을 작성해주세요.",
        verdict = verdict_label,
        strengths = strengths_desc,
        problems = problems_desc,
        suggestions = suggestions_desc,
    );

    let resp = llm
        .chat(
            LlmTask::OutfitExplanation,
            ChatRequest::new(vec![Message::user_text(user_prompt)]).system(system_prompt),
        )
        .await?;

    Ok(resp.text_or_empty().to_string())
}
