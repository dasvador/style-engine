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
        clothes.join(", ")
    };

    let occasion_text = occasion.unwrap_or("일상");
    let style_text = style_preference.unwrap_or("편한 스타일");

    let system_prompt = "당신은 패션 코디네이터 AI입니다. 날씨와 사용자의 옷장을 기반으로 오늘의 옷차림을 추천해주세요. 반드시 JSON 형식으로 응답하세요.";

    let user_prompt = format!(
        r#"현재 날씨:
- 기온: {temp}°C (체감: {feels}°C)
- 습도: {humidity}%
- 바람: {wind} km/h
- 날씨: {desc}

사용자의 옷장: {clothes}

상황: {occasion}
선호 스타일: {style}

다음 JSON 형식으로 응답하세요:
{{
  "recommendation": "전체 추천 요약 (2-3문장)",
  "outfit": [
    {{ "category": "상의", "name": "추천 아이템", "reason": "추천 이유" }},
    {{ "category": "하의", "name": "추천 아이템", "reason": "추천 이유" }}
  ],
  "weather_summary": "날씨 요약 한 줄",
  "tips": ["팁1", "팁2"]
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

pub async fn analyze_clothing_image(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<VisionAnalysisResult> {
    let system_prompt = r#"당신은 아메카지(아메리칸 캐주얼), 빈티지, 밀리터리, 워크웨어 남성 패션에 정통한 의류 전문가 AI입니다.
사용자가 업로드한 이미지를 분석하여 의류/신발 정보를 최대한 구체적으로 추출합니다.

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

## 스타일 태그 설명
- tone: 아이템의 전체 밝기 (밝음=화이트/연한색, 중간=그레이/카키, 어두움=블랙/네이비/다크계열)
- saturation: 색상 채도 (낮음=무채색/파스텔, 중간=자연색, 높음=선명한 색)
- style: 아이템이 속하는 대표 스타일 카테고리
- weight: 시각적 무게감 (가벼움=얇고 밝은, 중간=보통, 무거움=두껍고 어두운)
- role: 코디에서의 역할
  - 밥: 코디의 베이스/기본 아이템 (화이트 티, 인디고 데님, 그레이 팬츠 등)
  - 반찬: 코디의 포인트/강조 아이템 (러스트 셔츠, 로얄블루 자켓 등 눈에 띄는 것)
  - 약한반찬: 은은한 포인트 (올리브 자켓, 버건디 니트 등)
  - 연결템: 톤을 이어주는 브릿지 역할 (브라운 샴브레이, 카키 치노 등)
  - 구조템: 코디의 실루엣/구조를 잡아주는 아이템 (테일러드 자켓, 코트 등)
- color_temperature: 색상의 온도감 (warm=러스트/카멜/브라운, cool=네이비/그레이/블랙, neutral=화이트/베이지/카키)
- versatility: 활용도 (universal=어디든 OK, flexible=대부분 상황, situational=특정 상황, statement=존재감 강해 제한적)
- statement_level: 존재감 (1=무난, 5=강렬)
- formality_level: 격식 수준 (1=캐주얼, 5=포멀)
- texture_worlds: 아이템이 속하는 텍스처 월드 (복수 선택 가능)
  - workwear, military, tailoring, sweat, outdoor, minimal 중 해당하는 것 모두

사용자가 주로 수집하는 브랜드 (이미지에서 브랜드 특징이 보이면 적극 반영):
- 밀리터리 복각: The Real McCoy's, Buzz Rickson's, Bronson, Colimbo, orSlow
- 밀리터리 재해석: FreeWHEELERS, Warehouse, Pherrow's, Nigel Cabourn, Kaptain Sunshine
- 데님: Warehouse, Levi's, Fullcount, orSlow, Kapital
- 스웻셔츠: Loopwheeler, Champion, Warehouse, BARNS Outfitters
- 스니커: New Balance, Vans
- 기타: Polo Ralph Lauren, Standard California, Sugar Cane, Post Overalls

name 작성 규칙 (가장 중요):
- 형식: "색상 + 브랜드(가능시) + 소재 + 아이템명"
- 데님: 셀비지 여부, 온스, 핏, 워싱 정도를 반영 (예: "인디고 14oz 셀비지 데님 스트레이트", "Warehouse Lot.800 원워시 데님")
- 스웻셔츠: 소재(루프백/리버스위브), 넥라인, 무게감 반영 (예: "그레이 루프백 크루넥 스웻셔츠", "네이비 Champion 리버스위브 후디")
- 스니커: 브랜드, 모델명, 소재 (예: "그레이 New Balance 993 스웨이드 스니커", "블랙 Vans Old Skool 캔버스")
- 밀리터리: 모델명 정확히 (예: "올리브 백사틴 M-43 필드 자켓")
- 단순히 "청바지", "운동화" 같은 일반 이름은 절대 사용하지 마세요

기타 규칙:
- is_clothing: 의류, 신발, 가방, 모자, 벨트, 액세서리이면 true. 그 외 false.
- category: 상의, 하의, 아우터, 신발, 액세서리, 가방, 모자, 벨트 중 하나
- thickness: thin, medium, thick 중 하나
- seasons: 봄, 여름, 가을, 겨울 중 선택
- is_clothing이 true이면 name은 절대 null이 될 수 없음
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason 작성"#;

    let user_content = json!([
        {
            "type": "text",
            "text": "이 이미지를 분석하여 의류 정보를 추출해주세요."
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

/// Pass 1: Quick Vision analysis that returns a text description of the clothing
pub async fn analyze_clothing_pass1(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
) -> anyhow::Result<Pass1Result> {
    let system_prompt = r#"당신은 아메카지(아메리칸 캐주얼), 빈티지, 밀리터리, 워크웨어 패션 전문 감정사 AI입니다.
이미지에 보이는 의류/신발의 특징을 정밀하게 서술하세요.

아이템 종류에 따라 해당하는 항목을 포함하세요 (모르면 "확인 불가"):

[아우터/상의]
1. 칼라 형태, 2. 여밈 방식, 3. 포켓 수/종류, 4. 소재 질감과 무게감, 5. 기장, 6. 기타 디테일, 7. 색상

[하의/데님]
1. 핏(스트레이트/테이퍼/와이드/슬림), 2. 소재(셀비지 데님/논셀비지/치노/코듀로이), 3. 온스(두께감), 4. 색상/워싱 정도(원워시/생지/빈티지 워싱), 5. 디테일(체인스티치/히든리벳/아웃솔기), 6. 포켓 수

[신발/스니커]
1. 종류(스니커/부츠/로퍼), 2. 소재(스웨이드/캔버스/가죽/메시), 3. 솔 타입, 4. 색상, 5. 모델 특징

[스웻셔츠/니트]
1. 넥라인(크루넥/V넥/후드), 2. 소재(루프백/리버스위브/울), 3. 무게감, 4. 커프스/밑단 리브 유무, 5. 색상

반드시 JSON 형식으로 응답하세요:
{"description": "해당 아이템 종류에 맞는 항목을 포함한 상세 서술 (한국어, 200자 이상)"}"#;

    let user_content = json!([
        {
            "type": "text",
            "text": "이 의류 이미지의 외관적 특징을 상세히 서술해주세요."
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

/// Pass 2: Detailed analysis using RAG-retrieved reference context
pub async fn analyze_clothing_pass2(
    client: &reqwest::Client,
    api_key: &str,
    image_data_url: &str,
    references: &[ReferenceMatch],
) -> anyhow::Result<VisionAnalysisResult> {
    // Build reference context string
    let ref_context: String = references
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "참고자료 {}: {} (시대: {}, 스타일: {})\n{}",
                i + 1,
                r.name,
                r.era.as_deref().unwrap_or("N/A"),
                r.style.as_deref().unwrap_or("N/A"),
                r.description,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system_prompt = format!(
        r#"당신은 아메카지(아메리칸 캐주얼), 빈티지, 밀리터리, 워크웨어 패션 전문 감정사 AI입니다.

## 후보 레퍼런스 (유사도 순)
{ref_context}

## 판별 절차 (반드시 따를 것)
1. 이미지에서 아이템 종류를 먼저 판단하세요 (아우터/상의/하의/신발 등)
2. 후보 레퍼런스 중 같은 종류의 아이템과 비교하세요
3. 일치하는 레퍼런스가 있으면 그 모델명을 사용하세요
4. 일치하는 레퍼런스가 없으면 이미지를 직접 관찰하여 구체적인 이름을 작성하세요

## name 작성 규칙 (가장 중요)
- 형식: "색상 + 소재 + 아이템명"
- 브랜드를 특정할 수 있으면 포함
- 레퍼런스와 매칭되면 레퍼런스의 모델명 사용
- 매칭 안 되면 자체 관찰로 구체적 이름 작성
- 예시: "올리브 백사틴 M-43 필드 자켓", "인디고 14oz 셀비지 데님", "그레이 루프백 크루넥 스웻셔츠", "그레이 뉴발란스 990v3 스니커"
- name은 절대 null이 될 수 없음

## JSON 응답 형식
{{
  "is_clothing": true,
  "name": "색상 + 소재 + 아이템명",
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

## 스타일 태그 설명
- tone: 아이템의 전체 밝기 (밝음=화이트/연한색, 중간=그레이/카키, 어두움=블랙/네이비/다크계열)
- saturation: 색상 채도 (낮음=무채색/파스텔, 중간=자연색, 높음=선명한 색)
- style: 아이템이 속하는 대표 스타일 카테고리
- weight: 시각적 무게감 (가벼움=얇고 밝은, 중간=보통, 무거움=두껍고 어두운)
- role: 코디에서의 역할
  - 밥: 베이스/기본 아이템 (화이트 티, 인디고 데님 등)
  - 반찬: 포인트/강조 아이템 (러스트 셔츠, 로얄블루 자켓 등)
  - 약한반찬: 은은한 포인트 (올리브 자켓, 버건디 니트 등)
  - 연결템: 톤 브릿지 (브라운 샴브레이, 카키 치노 등)
  - 구조템: 실루엣/구조를 잡아주는 아이템 (테일러드 자켓, 코트 등)
- color_temperature: warm=러스트/카멜/브라운, cool=네이비/그레이/블랙, neutral=화이트/베이지/카키
- versatility: universal=어디든, flexible=대부분, situational=특정 상황, statement=존재감 강해 제한적
- statement_level: 1=무난 ~ 5=강렬
- formality_level: 1=캐주얼 ~ 5=포멀
- texture_worlds: workwear/military/tailoring/sweat/outdoor/minimal 중 복수 선택

- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason 작성"#
    );

    let user_content = json!([
        {
            "type": "text",
            "text": "이 이미지를 분석하여 의류 정보를 추출해주세요. 위의 레퍼런스를 참고하여 정확한 모델명을 식별하세요."
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
    // Pass 1: Get a text description of the image
    tracing::info!("RAG Pass 1: Getting image description...");
    let pass1 = analyze_clothing_pass1(client, api_key, image_data_url).await?;
    tracing::info!("RAG Pass 1 result: {}", &pass1.description);

    // Embed + Retrieve: Find top-5 similar references
    let references = embedding_service.search(&pass1.description, 5).await?;

    let top_similarity = references.first().map(|r| r.similarity).unwrap_or(0.0);
    let ref_names: Vec<&str> = references.iter().map(|r| r.name.as_str()).collect();
    tracing::info!(
        "RAG retrieved {} references (top sim={:.3}): {:?}",
        references.len(),
        top_similarity,
        ref_names
    );

    // If top similarity is too low, references are irrelevant → fallback to broad analysis
    if top_similarity < 0.5 {
        tracing::info!("RAG similarity too low ({:.3}), falling back to general analysis", top_similarity);
        return analyze_clothing_image(client, api_key, image_data_url).await;
    }

    // Pass 2: Detailed analysis with reference context
    tracing::info!("RAG Pass 2: Detailed analysis with reference context...");
    let result = analyze_clothing_pass2(client, api_key, image_data_url, &references).await?;

    Ok(result)
}

/// Generate natural language explanation for outfit evaluation
pub async fn generate_outfit_explanation(
    client: &reqwest::Client,
    api_key: &str,
    items_desc: &str,
    score: i32,
    problems_desc: &str,
    suggestions_desc: &str,
) -> anyhow::Result<String> {
    let system_prompt = r#"당신은 아메카지/빈티지/밀리터리 패션에 정통한 스타일 코치입니다.
사용자의 코디 조합을 평가한 결과를 바탕으로, 자연스럽고 친근한 한국어로 설명해주세요.

규칙:
- 2~4문장으로 간결하게
- 좋은 점을 먼저, 문제점과 개선안을 뒤에
- 구체적인 아이템명을 언급하며 설명
- "밥/반찬" 비유를 자연스럽게 활용 가능
- 점수는 언급하지 마세요"#;

    let user_prompt = format!(
        "코디 구성:\n{items_desc}\n\n점수: {score}/100\n\n감지된 문제:\n{problems_desc}\n\n개선 제안:\n{suggestions_desc}\n\n위 내용을 바탕으로 코디 평가를 자연스럽게 설명해주세요."
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
