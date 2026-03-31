use std::sync::Arc;

use anyhow::Context;
use serde_json::json;

use crate::models::clothing::{Pass1Result, VisionAnalysisResult};
use crate::models::recommendation::AiRecommendation;
use crate::models::reference::ReferenceMatch;
use crate::models::weather::CurrentWeather;
use crate::services::embedding::EmbeddingService;

pub async fn get_outfit_recommendation(
    client: &reqwest::Client,
    api_key: &str,
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

    // [개선 5] 구조화된 메타데이터 기반 추천 + 역할/색온도 규칙 강화
    let system_prompt = r#"당신은 아메카지/빈티지/밀리터리 패션에 정통한 스타일 코디네이터 AI입니다.
날씨와 사용자의 옷장 메타데이터를 기반으로 오늘의 옷차림을 추천합니다.

## 중요: 당신의 역할
- 후보 조합을 제안하는 역할입니다.
- 최종 점수/판정은 별도 규칙 엔진이 수행합니다.
- 아래 규칙을 따라 규칙 엔진에서 높은 점수를 받을 조합을 만드세요.

## 코디 조합 규칙 (반드시 따를 것)
1. 밥/반찬 밸런스: 반찬(포인트) 아이템은 최대 1개. 나머지는 밥(베이스)/연결템으로 구성.
2. 색상 대비: 같은 색상끼리 매칭하지 마세요. (예: 올리브 아우터 + 올리브 하의 ❌)
3. 색온도 밸런스: warm 아이템만으로 구성하지 마세요. cool/neutral을 반드시 섞으세요.
4. 톤 대비: 밝은 아이템과 어두운 아이템을 함께 배치하세요.
5. 이너 규칙: 아우터가 있으면 이너(상의)는 밥/연결템 역할이어야 합니다.
6. 하의 규칙: 하의는 밥/구조템 역할이 좋습니다. 하의에 반찬을 넣지 마세요.
7. 세계관 규칙: 밀리터리 아우터 + 올리브/카키/카멜 하의는 군복처럼 보입니다. 인디고 데님/차콜/블랙 하의를 우선하세요.
8. 스타일 충돌: 워크 2개, 밀리터리+포멀 등 강한 스타일이 겹치지 않게 하세요.
9. 기온별 아우터:
   - 15°C 이하: 아우터 필수
   - 20°C 이하: 아우터 권장
   - 25°C 이상: 아우터 불필요
10. 코디는 최소 상의+하의+아우터(필요시) 3피스로 구성하세요.

## 옷장 데이터 형식
각 아이템은 "이름 | 카테고리 | 색상 | 톤 | 색온도 | 역할 | 스타일 | 무게감 | 격식도" 형식입니다.
이 메타데이터를 활용하여 규칙에 맞는 조합을 만드세요.

반드시 JSON 형식으로 응답하세요."#;

    let user_prompt = format!(
        r#"현재 날씨:
- 기온: {temp}°C (체감: {feels}°C)
- 습도: {humidity}%
- 바람: {wind} km/h
- 날씨: {desc}

사용자의 옷장:
{clothes}

상황: {occasion}
선호 스타일: {style}

위 옷장에서 아이템을 선택하여 코디를 구성해주세요.
name은 반드시 옷장에 있는 아이템의 정확한 이름(| 앞부분)을 사용하세요.

다음 JSON 형식으로 응답하세요:
{{
  "recommendation": "전체 추천 요약 (2-3문장, 왜 이 조합이 좋은지 역할/색온도/톤 관점에서 설명)",
  "outfit": [
    {{ "category": "상의", "name": "옷장에 있는 정확한 아이템명", "reason": "추천 이유 (역할과 색 조합 관점)" }},
    {{ "category": "하의", "name": "옷장에 있는 정확한 아이템명", "reason": "추천 이유" }}
  ],
  "weather_summary": "날씨 요약 한 줄",
  "tips": ["스타일 팁1", "스타일 팁2"]
}}"#,
        temp = weather.temperature,
        feels = weather.apparent_temperature,
        humidity = weather.humidity,
        wind = weather.wind_speed,
        desc = weather.weather_description,
        clothes = clothes_list,
        occasion = occasion_text,
        style = style_text,
    );

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.7,
        "max_tokens": 1000
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Failed to call OpenAI API")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error ({}): {}", status, text);
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse OpenAI response")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in OpenAI response")?;

    let recommendation: AiRecommendation =
        serde_json::from_str(content).context("Failed to parse AI recommendation JSON")?;

    Ok(recommendation)
}

// [개선 1] 브랜드/모델 환각 방지 + [개선 4] 태그는 초기 추정값임을 명시
pub async fn analyze_clothing_image(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<VisionAnalysisResult> {
    let system_prompt = r#"당신은 아메카지(아메리칸 캐주얼), 빈티지, 밀리터리, 워크웨어 남성 패션에 정통한 의류 전문가 AI입니다.
사용자가 업로드한 이미지를 분석하여 의류/신발 정보를 추출합니다.

## 핵심 원칙
- 이미지에서 **직접 확인 가능한 특징만** 서술하세요.
- 브랜드/모델명은 **로고, 라벨, 고유 디테일이 명확히 보일 때만** 포함하세요.
- 확신할 수 없는 브랜드는 절대 추측하지 마세요.
- 브랜드를 모르면 "색상 + 소재 + 아이템명" 형식으로 충분합니다.

## name 작성 규칙
- 형식: "색상 + 소재(보이면) + 아이템명"
- 브랜드 확실할 때: "올리브 Buzz Rickson's B-15 플라이트 자켓"
- 브랜드 불확실: "올리브 나일론 플라이트 자켓" (브랜드 생략)
- 데님: 셀비지 여부, 워싱 정도 (예: "인디고 셀비지 데님 스트레이트")
- 스니커: 모델 특징이 보이면 포함 (예: "그레이 메시 러닝 스니커")
- 단순히 "청바지", "운동화" 같은 일반 이름은 사용하지 마세요

## 스타일 태그 (초기 추정값 — 사용자가 나중에 수정할 수 있음)
아래 태그는 이미지에서 보이는 특징을 기반으로 최선의 추정을 하세요:

반드시 다음 JSON 형식으로 응답하세요:
{
  "is_clothing": true,
  "name": "색상 + 소재 + 아이템명",
  "category": "상의/하의/아우터/신발/액세서리/가방/모자/벨트 중 하나",
  "color": "색상",
  "thickness": "thin/medium/thick 중 하나",
  "seasons": ["봄", "여름", "가을", "겨울 중 선택"],
  "tone": "밝음/중간/어두움",
  "saturation": "낮음/중간/높음",
  "style": "베이직/워크/밀리터리/포멀/스포츠",
  "weight": "가벼움/중간/무거움",
  "role": "밥/반찬/약한반찬/연결템/구조템",
  "color_temperature": "warm/cool/neutral",
  "versatility": "universal/flexible/situational/statement",
  "statement_level": 1~5,
  "formality_level": 1~5,
  "texture_worlds": ["workwear/military/tailoring/sweat/outdoor/minimal 중 복수"],
  "rejection_reason": null
}

## 태그 기준
- tone: 밝음=화이트/연한색, 중간=그레이/카키/올리브, 어두움=블랙/네이비/다크
- role: 밥=베이스(화이트 티, 데님), 반찬=포인트(선명한 색), 약한반찬=은은한 포인트, 연결템=브릿지, 구조템=실루엣
- color_temperature: warm=러스트/카멜/브라운, cool=네이비/그레이/블랙, neutral=화이트/베이지/카키
- texture_worlds: 복수 선택 가능

## 기타
- is_clothing: 의류/신발/가방/모자/벨트/액세서리이면 true
- is_clothing이 false이면 name~seasons는 null, rejection_reason 작성"#;

    let user_content = json!([
        {
            "type": "text",
            "text": "이 이미지를 분석하여 의류 정보를 추출해주세요. 브랜드는 확실할 때만 포함하세요."
        },
        {
            "type": "image_url",
            "image_url": {
                "url": image_data_url,
                "detail": "high"
            }
        }
    ]);

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_content }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.3,
        "max_tokens": 500
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Failed to call OpenAI Vision API")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI Vision API error ({}): {}", status, text);
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse OpenAI Vision response")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in OpenAI Vision response")?;

    let result: VisionAnalysisResult =
        serde_json::from_str(content).context("Failed to parse Vision analysis JSON")?;

    Ok(result)
}

// [개선 2] Pass1: 변별력 있는 시각 특징 우선, 길이보다 질
pub async fn analyze_clothing_pass1(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<Pass1Result> {
    let system_prompt = r#"당신은 아메카지, 빈티지, 밀리터리, 워크웨어 패션 전문 감정사 AI입니다.

## 목적
이미지에서 이 아이템을 **다른 비슷한 아이템과 구분할 수 있는** 핵심 시각 특징을 서술하세요.
서술은 나중에 텍스트 임베딩으로 변환되어 레퍼런스 DB와 매칭됩니다.

## 서술 원칙
1. **변별력 우선**: 일반적인 설명("자켓이다")보다 구분 가능한 특징("4개의 대형 플랩 포켓, 에폴렛, 히든 후드")을 먼저 쓰세요.
2. **구조적 특징**: 칼라 형태, 여밈 방식, 포켓 구조, 디테일 순서로 서술하세요.
3. **추측 금지**: 보이지 않는 특징은 "확인 불가"로 표시하세요.
4. **브랜드 언급 금지**: 이 단계에서는 브랜드/모델명을 추측하지 마세요. 순수하게 시각적 특징만 서술합니다.

## 아이템별 핵심 관찰 항목

[아우터/상의] 칼라 형태 → 여밈 방식 → 포켓 수/종류 → 소재 질감 → 무게감 → 기장 → 색상
[하의/데님] 핏 → 소재 → 두께감 → 색상/워싱 → 디테일(솔기/리벳) → 포켓
[신발] 종류 → 소재 → 솔 → 색상 → 모델 특징
[스웻/니트] 넥라인 → 소재감 → 무게감 → 리브 유무 → 색상

반드시 JSON으로 응답하세요:
{"description": "변별력 있는 시각 특징 중심 서술 (한국어, 150~300자)"}"#;

    let user_content = json!([
        {
            "type": "text",
            "text": "이 의류의 시각적 특징을 서술해주세요. 브랜드는 추측하지 말고, 다른 아이템과 구분할 수 있는 구조적 특징에 집중하세요."
        },
        {
            "type": "image_url",
            "image_url": {
                "url": image_data_url,
                "detail": "high"
            }
        }
    ]);

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_content }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.3,
        "max_tokens": 500
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Failed to call OpenAI Vision API (pass1)")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI Vision API error (pass1) ({}): {}", status, text);
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse OpenAI Vision response (pass1)")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in OpenAI Vision response (pass1)")?;

    let result: Pass1Result =
        serde_json::from_str(content).context("Failed to parse pass1 JSON")?;

    Ok(result)
}

// [개선 3] Pass2: 레퍼런스는 강한 시각적 일치일 때만 사용
pub async fn analyze_clothing_pass2(
    client: &reqwest::Client,
    api_key: &str,
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
        r#"당신은 아메카지, 빈티지, 밀리터리, 워크웨어 패션 전문 감정사 AI입니다.

## 후보 레퍼런스 (유사도 순)
{ref_context}

## 판별 절차 (엄격하게 따를 것)
1. 이미지에서 아이템 종류를 먼저 판단하세요 (아우터/상의/하의/신발 등)
2. 후보 레퍼런스와 비교하되, **핵심 구조적 특징이 대부분 일치할 때만** 레퍼런스 모델명을 사용하세요
3. 부분적으로만 비슷한 경우 (예: 색상만 같고 구조가 다름) → 레퍼런스 사용 금지
4. 일치하는 레퍼런스가 없으면 이미지를 직접 관찰하여 구체적인 이름을 작성하세요

## 레퍼런스 매칭 기준
- ✅ 매칭 OK: 칼라+포켓+여밈+기장이 모두 일치
- ❌ 매칭 금지: 색상만 비슷, 카테고리만 같음, 부분적 유사

## name 작성 규칙
- 형식: "색상 + 소재(보이면) + 아이템명"
- 레퍼런스 매칭 시: 레퍼런스의 모델명 사용 (예: "올리브 백사틴 M-65 필드 자켓")
- 매칭 안 될 때: 직접 관찰로 구체적 이름 (예: "카키 코튼 유틸리티 자켓")
- 브랜드는 로고/라벨이 보일 때만 포함
- name은 절대 null이 될 수 없음

## 스타일 태그 (초기 추정값)
아래 태그는 규칙 엔진의 입력으로 사용됩니다. 최선의 추정을 하되, 사용자가 수정할 수 있습니다.

## JSON 응답 형식
{{
  "is_clothing": true,
  "name": "색상 + 소재 + 아이템명",
  "category": "상의/하의/아우터/신발/액세서리/가방/모자/벨트",
  "color": "색상",
  "thickness": "thin/medium/thick",
  "seasons": ["계절"],
  "tone": "밝음/중간/어두움",
  "saturation": "낮음/중간/높음",
  "style": "베이직/워크/밀리터리/포멀/스포츠",
  "weight": "가벼움/중간/무거움",
  "role": "밥/반찬/약한반찬/연결템/구조템",
  "color_temperature": "warm/cool/neutral",
  "versatility": "universal/flexible/situational/statement",
  "statement_level": 1~5,
  "formality_level": 1~5,
  "texture_worlds": ["workwear/military/tailoring/sweat/outdoor/minimal"],
  "rejection_reason": null
}}

## 태그 기준
- role: 밥=베이스, 반찬=포인트, 약한반찬=은은한 포인트, 연결템=브릿지, 구조템=실루엣
- color_temperature: warm=러스트/카멜/브라운, cool=네이비/그레이/블랙, neutral=화이트/베이지/카키

- is_clothing이 false이면 name~seasons는 null, rejection_reason 작성"#
    );

    let user_content = json!([
        {
            "type": "text",
            "text": "이 이미지를 분석하세요. 레퍼런스는 구조적 특징이 대부분 일치할 때만 참고하고, 부분적 유사성만으로는 레퍼런스 모델명을 사용하지 마세요."
        },
        {
            "type": "image_url",
            "image_url": {
                "url": image_data_url,
                "detail": "high"
            }
        }
    ]);

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_content }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.2,
        "max_tokens": 500
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Failed to call OpenAI Vision API (pass2)")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI Vision API error (pass2) ({}): {}", status, text);
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse OpenAI Vision response (pass2)")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in OpenAI Vision response (pass2)")?;

    let result: VisionAnalysisResult =
        serde_json::from_str(content).context("Failed to parse pass2 analysis JSON")?;

    Ok(result)
}

/// Full 2-pass RAG pipeline: pass1 → embed → retrieve → pass2
pub async fn analyze_clothing_image_with_rag(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
    embedding_service: &Arc<EmbeddingService>,
) -> anyhow::Result<VisionAnalysisResult> {
    tracing::info!("RAG Pass 1: Getting image description...");
    let pass1 = analyze_clothing_pass1(client, api_key, image_data_url).await?;
    tracing::info!("RAG Pass 1 result: {}", &pass1.description);

    let references = embedding_service.search(&pass1.description, 5).await?;

    let top_similarity = references.first().map(|r| r.similarity).unwrap_or(0.0);
    let ref_names: Vec<&str> = references.iter().map(|r| r.name.as_str()).collect();
    tracing::info!(
        "RAG retrieved {} references (top sim={:.3}): {:?}",
        references.len(),
        top_similarity,
        ref_names
    );

    if top_similarity < 0.5 {
        tracing::info!("RAG similarity too low ({:.3}), falling back to general analysis", top_similarity);
        return analyze_clothing_image(client, api_key, image_data_url).await;
    }

    tracing::info!("RAG Pass 2: Detailed analysis with reference context...");
    let result = analyze_clothing_pass2(client, api_key, image_data_url, &references).await?;

    Ok(result)
}

// [개선 6] 설명 생성: strengths를 명시적으로 전달
pub async fn generate_outfit_explanation(
    client: &reqwest::Client,
    api_key: &str,
    items_desc: &str,
    score: i32,
    problems_desc: &str,
    suggestions_desc: &str,
) -> anyhow::Result<String> {
    let system_prompt = r#"당신은 아메카지/빈티지/밀리터리 패션에 정통한 스타일 코치입니다.

## 역할
규칙 엔진이 이미 판정한 결과를 **자연어로 풀어서 설명**하는 역할입니다.
점수/판정을 직접 내리지 마세요. 제공된 강점/문제점/제안을 자연스러운 문장으로 변환하세요.

## 규칙
- 2~4문장으로 간결하게
- 강점이 있으면 반드시 먼저 언급 (제공된 강점 정보를 활용)
- 문제점과 개선안은 그 뒤에
- 구체적인 아이템명을 언급
- "밥/반찬" 비유를 자연스럽게 활용 가능
- 점수 숫자는 언급하지 마세요
- 규칙 엔진이 감지하지 않은 문제를 추가하지 마세요"#;

    let user_prompt = format!(
        "코디 구성:\n{items_desc}\n\n점수: {score}/95\n\n평가 결과(강점 포함):\n{problems_desc}\n\n개선 제안:\n{suggestions_desc}\n\n위 평가 결과를 자연스러운 한국어 문장으로 설명해주세요. 강점이 있으면 반드시 먼저 언급하세요."
    );

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "temperature": 0.7,
        "max_tokens": 500
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Failed to call OpenAI API for outfit explanation")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error (explanation) ({}): {}", status, text);
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse OpenAI response (explanation)")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in OpenAI response (explanation)")?;

    Ok(content.to_string())
}
