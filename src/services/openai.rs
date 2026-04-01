use std::sync::Arc;

use anyhow::Context;
use serde_json::json;

use crate::models::clothing::{Pass1Result, VisionAnalysisResult};
use crate::models::recommendation::AiRecommendation;
use crate::models::reference::ReferenceMatch;
use crate::models::weather::CurrentWeather;
use crate::services::embedding::EmbeddingService;

// ─── 1) get_outfit_recommendation ───

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

    let system_prompt = r#"당신은 아메카지/빈티지/밀리터리 패션에 익숙한 코디 보조 AI입니다.

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
4. 하의는 가능하면 안정적인 역할(밥/구조템/연결템) 아이템을 우선 선택하세요.
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

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "response_format": { "type": "json_object" },
        "temperature": 0.4,
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

// ─── 2) analyze_clothing_image (fallback, no RAG) ───

pub async fn analyze_clothing_image(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<VisionAnalysisResult> {
    let system_prompt = r#"당신은 아메카지, 빈티지, 밀리터리, 워크웨어 남성 패션에 익숙한 의류 분석 AI입니다.
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
  "thickness": "두께",
  "seasons": ["계절1", "계절2"],
  "tone": "밝음/중간/어두움 중 하나",
  "saturation": "낮음/중간/높음 중 하나",
  "style": "베이직/워크/밀리터리/포멀/스포츠 중 하나",
  "weight": "가벼움/중간/무거움 중 하나",
  "role": "밥/반찬/약한반찬/연결템/구조템 중 하나",
  "color_temperature": "warm/cool/neutral 중 하나",
  "versatility": "universal/flexible/situational/statement 중 하나",
  "statement_level": 1~5 사이 정수,
  "formality_level": 1~5 사이 정수,
  "texture_worlds": ["해당하는 텍스처 월드 모두 선택"],
  "rejection_reason": null
}

추가 규칙:
- is_clothing이 true이면 name은 null이면 안 됩니다.
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요.
- texture_worlds는 workwear, military, tailoring, sweat, outdoor, minimal 중 복수 선택 가능.
- category는 상의, 하의, 아우터, 신발, 액세서리, 가방, 모자, 벨트 중 하나입니다."#;

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

// ─── 3) analyze_clothing_pass1 ───

pub async fn analyze_clothing_pass1(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<Pass1Result> {
    let system_prompt = r#"당신은 아메카지, 빈티지, 밀리터리, 워크웨어 패션 전문 감정사 AI입니다.
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
{"description": "한국어로 작성한 상세 서술. 180~300자 내외 권장, 단 식별 가능한 특징을 우선"}"#;

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

// ─── 4) analyze_clothing_pass2 ───

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
  "role": "밥/반찬/약한반찬/연결템/구조템 중 하나",
  "color_temperature": "warm/cool/neutral 중 하나",
  "versatility": "universal/flexible/situational/statement 중 하나",
  "statement_level": 1~5,
  "formality_level": 1~5,
  "texture_worlds": ["해당하는 텍스처 월드 모두"],
  "rejection_reason": null
}}

추가 규칙:
- role과 versatility는 일반적인 활용성 기준의 1차 추정치입니다.
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요."#
    );

    let user_content = json!([
        {
            "type": "text",
            "text": "이 이미지를 분석하여 의류 정보를 추출해주세요. 위 레퍼런스는 참고하되, 핵심 특징이 충분히 일치할 때만 특정 모델명을 사용하세요."
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

// ─── 5) generate_outfit_explanation — 시그니처 변경: score 제거, verdict_label/strengths 분리 ───

pub async fn generate_outfit_explanation(
    client: &reqwest::Client,
    api_key: &str,
    items_desc: &str,
    verdict_label: &str,
    strengths_desc: &str,
    problems_desc: &str,
    suggestions_desc: &str,
) -> anyhow::Result<String> {
    let system_prompt = r#"당신은 아메카지/빈티지/밀리터리 패션에 익숙한 스타일 코치입니다.
이미 결정된 평가 결과를 사용해 자연스럽고 친근한 한국어 설명문을 작성하세요.

중요 원칙:
- 당신은 코디를 새로 판단하지 않습니다.
- 입력으로 주어진 강점, 문제점, 개선 제안을 자연스럽게 풀어 설명만 합니다.
- 규칙 엔진의 결론을 바꾸거나 새로운 문제를 만들어내지 마세요.
- 2~4문장으로 간결하게 작성하세요.
- 좋은 점을 먼저, 아쉬운 점과 개선안을 뒤에 배치하세요.
- 구체적인 아이템명을 언급하세요.
- "밥/반찬" 비유는 자연스러울 때만 사용하세요.
- 점수 숫자는 언급하지 마세요."#;

    let user_prompt = format!(
        "코디 구성:\n{items_desc}\n\n판정: {verdict}\n\n강점:\n{strengths}\n\n문제점:\n{problems}\n\n개선 제안:\n{suggestions}\n\n위 내용을 바탕으로 자연스럽고 짧은 한국어 설명을 작성해주세요.",
        verdict = verdict_label,
        strengths = strengths_desc,
        problems = problems_desc,
        suggestions = suggestions_desc,
    );

    let body = json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "temperature": 0.5,
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
