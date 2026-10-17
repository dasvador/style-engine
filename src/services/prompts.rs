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
    /// 색 다양성 무드(`mood_palette::targets`)에서 목록의 색 있는 상의·아우터("이름(계열)").
    /// 다른 무드는 비어 있다.
    pub colour_items: Vec<String>,
}

/// 후보 생성의 기본 원칙 3~5. 무드가 없거나 글램이 아닌 무드에 쓴다.
const DEFAULT_PRINCIPLES: &str = "3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너는 중립적이고 활용도 높은 아이템 우선.";

/// 색 다양성을 다루는 성별·무드(`mood_palette::targets` — 여성 스트리트·스포티 캐주얼, 남성
/// 아메카지·프레피)의 원칙 5. 관찰: 2026-10-17 감사에서 남성 프레피 초안 15개는 후보 목록에
/// 하늘색 셔츠가 있어도 기본색만 골랐다. 추론: 기본 원칙 5("이너는 중립적이고 활용도 높은
/// 아이템 우선")가 색 있는 셔츠·후디·트랙재킷을 밀어내는 한 원인으로 보인다 — 이 문장만의
/// 효과를 따로 잰 실험은 없다. 3·4 는 그대로 두고 5 만 장르의 색 조합으로 바꾼다. "기본색이어야
/// 안정적" 같은 공식도, "모두 컬러" 같은 공식도 두지 않는다.
fn palette_principles(mood: crate::models::style_vocab::StyleGenre) -> Option<&'static str> {
    use crate::models::style_vocab::StyleGenre as G;
    Some(match mood {
        G::Street => "3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너와 아우터에도 색을 쓸 수 있습니다. 스트리트는 그래픽 티·후디·봄버·코치 재킷의 색을 데님·블랙·그레이 하의가 받치는 조합이 자연스럽습니다. 색 없는 조합도 스트리트입니다.",
        G::SportyCasual => "3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너와 아우터에도 색을 쓸 수 있습니다. 스포티 캐주얼은 트랙재킷·윈드브레이커·기능성 탑의 색을 그레이·블랙·화이트 하의가 받치는 조합이 자연스럽습니다. 색 없는 조합도 스포티 캐주얼입니다.",
        G::Amekaji => "3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너와 아우터에도 색을 쓸 수 있습니다. 아메카지는 플란넬·워크 셔츠·코듀로이 같은 옷의 붉은 계열·러스트·블루를 데님·치노·카키·브라운이 받치는 조합이 자연스럽습니다. 색 없는 조합도 아메카지입니다.",
        G::Preppy => "3. 강한 포인트 아이템은 1개 이하로 유지하세요.
4. 하의는 안정적인 역할(베이스/구조템/연결템) 우선.
5. 이너와 아우터에도 색을 쓸 수 있습니다. 프레피는 하늘색·블루 계열 셔츠 같은 색 있는 상의에 네이비·치노·그레이 하의를 맞추는 조합이 자연스럽습니다. 색 없는 조합도 프레피입니다.",
        _ => return None,
    })
}

/// 색 다양성 무드의 후보 방향. 목록에 색 있는 상의·아우터가 있을 때만 넣고, 그 옷들을 이름으로
/// 알려 준다. 관찰: 2026-10-17 남성 프레피 실측에서 "목록에 기본색이 아닌 색의 옷이 있으면"
/// 이라고만 쓴 문장으로는 초안 12개가 하늘색 셔츠를 한 번도 고르지 않았고, 이름 목록을 넣은 뒤
/// 3회 모두 최종 카드에 블루 셔츠가 나왔다(각 3회의 작은 표본). 없는 색을 만들지 않는다.
fn palette_tasks(
    mood: Option<crate::models::style_vocab::StyleGenre>,
    colour_items: &[String],
) -> String {
    if colour_items.is_empty() {
        return String::new();
    }
    // 남성 아메카지: 목록에 색 계열이 둘 이상이면 색 후보끼리 다른 계열을 쓰게 한다. 관찰:
    // 2026-10-17 최종 실측 3회의 색 초안 5개가 모두 같은 계열(러스트 셔츠·러스트 재킷)이었다 —
    // "후보마다 다른 옷" 은 같은 색의 다른 옷으로 채워진다. 계열은 목록이 이미 "이름(계열)" 로
    // 알려 준다. 다른 무드 문장은 그대로다.
    let families: std::collections::HashSet<&str> = colour_items
        .iter()
        .filter_map(|n| n.rsplit_once('(').map(|(_, f)| f.trim_end_matches(')')))
        .collect();
    let distinct = if mood == Some(crate::models::style_vocab::StyleGenre::Amekaji)
        && families.len() >= 2
    {
        "- 색: 색 후보끼리는 서로 다른 색 계열(괄호 안)을 쓰세요 — 같은 계열의 다른 옷 두 개로 채우지 마세요. 최근 추천된 아이템에 있는 색 옷을 다시 고르기보다 아직 쓰지 않은 계열을 먼저 쓰세요.\n"
    } else {
        ""
    };
    format!(
        "- 색: 이번 목록의 색 있는 상의·아우터는 {} 입니다. 5개 중 2개 이상은 이 중 하나를 상의나 아우터에 넣고, 가능하면 후보마다 다른 옷을 쓰세요. 기본색(블랙·화이트·그레이·베이지·브라운·네이비·데님·올리브·카키)만으로 된 후보도 1개 이상 남기세요. 가방·신발만 색이 있는 조합은 색 후보로 세지 않습니다.\n",
        colour_items.join(", ")
    ) + distinct
}

/// 글램의 원칙 3~5. 기본 원칙의 "중립적인 이너·안정적인 하의" 와 부딪쳐서 바꿔 끼운다.
/// 중심은 옷 한 곳 — 어느 슬롯이어도 된다. 가방·신발은 중심이 되지 못한다.
const GLAM_PRINCIPLES: &str = "3. 각 후보는 옷(상의·하의·아우터) 한 곳에 시선이 모이는 중심을 두세요. 중심은 빛을 받는 소재(새틴·벨벳·메탈릭)나 형태(원숄더·드레이프·비대칭), 선명한 색으로 만듭니다. 노출이나 광택이 꼭 있어야 하는 것은 아닙니다.
4. 중심이 아닌 옷은 담백한 아이템으로 중심을 받치세요. 옷 두 곳을 동시에 강하게 만들지 마세요(예: 강한 상의 + 강한 아우터).
5. 가방·신발은 작은 보조 포인트일 뿐 중심이 되지 못합니다. 기본 티셔츠·데님에 가방만 빛나는 조합은 글램 후보로 내지 마세요.";

/// 글램 후보의 방향. 날씨와 옷장이 허락하는 만큼만 — 없는 방향을 억지로 만들지 않는다.
const GLAM_TASKS: &str = "- 글램: 날씨와 후보 목록이 허락하면 5개 중 상의가 중심인 후보, 하의가 중심인 후보, 아우터가 중심인 후보를 각각 1개 이상 넣으세요. 그런 옷이 목록에 없으면 억지로 만들지 마세요.
- 글램: 각 후보의 \"focus\" 에 시선이 모이는 옷의 슬롯(상의·하의·아우터 중 하나)을 적으세요.
";

/// 엣지의 원칙 3~5. 기본 원칙의 "강한 포인트 1개 이하·중립적인 이너·안정적인 하의" 는
/// 형태가 있는 상의·하의를 처음부터 밀어낸다. 그렇다고 "중심은 딱 한 곳" 같은 새 공식을
/// 두지도 않는다 — 형태와 소재 대비가 여러 곳에서 어울리는 것도 엣지다.
const EDGY_PRINCIPLES: &str = "3. 각 후보는 옷(상의·하의·아우터)의 형태나 소재 대비로 특징이 드러나게 하세요. 형태는 비대칭 넥·헴, 각진 어깨, 배럴·와이드 레그, 박시한 크롭처럼 재단에서 나오고, 소재 대비는 가죽과 니트·울·데님을 함께 입는 데서 나옵니다. 특징이 여러 곳에 있어도 서로 어울리면 괜찮습니다.
4. 실루엣의 비율을 정하세요 — 몸에 맞는 상의와 넓거나 묵직한 하의, 박시한 아우터와 날렵한 하의처럼. 후보마다 하의 종류(스커트·와이드·배럴·데님)와 아우터 종류를 바꿔 같은 틀(예: 라이더 + 검은 스키니)을 반복하지 마세요.
5. 가방·신발은 코디를 거들 뿐 중심이 되지 못합니다. 기본 티셔츠·데님에 가죽 가방이나 부츠만 더한 조합은 엣지 후보로 내지 마세요. 후디·조거·운동화 중심(스트리트)이나 단정한 정장 세트(모던 시크)로 흐르지 마세요.";

/// 엣지 후보의 방향. 날씨와 옷장이 허락하는 만큼만.
const EDGY_TASKS: &str = "- 엣지: 날씨와 후보 목록이 허락하면 5개 중 상의의 형태가 중심인 후보, 하의의 형태나 가죽이 중심인 후보, 아우터의 형태가 중심인 후보를 각각 1개 이상 넣으세요. 그런 옷이 목록에 없으면 억지로 만들지 마세요.
- 엣지: 각 후보의 \"focus\" 에 특징이 드러나는 옷의 슬롯(상의·하의·아우터 중 하나)을 적으세요.
";

/// 엣지의 색 다양성. 밝은색을 싫어한다고 적은 사용자에게는 넣지 않는다
/// (`user_style_profile.dislikes_bright_colors`).
const EDGY_COLOUR_TASKS: &str = "- 엣지: 목록에 무채색(블랙·차콜·그레이·화이트)이 아닌 색이 뚜렷한 상의나 아우터가 있으면, 5개 중 2개 이상은 그런 옷을 상의나 아우터에 넣고 후보마다 다른 색을 쓰세요. 나머지 옷은 그 색과 어울리게 고르세요. 무채색만으로 된 엣지 후보도 1개 이상 남기세요. 가방·신발만 색이 있는 조합은 색 후보로 세지 않습니다.
- 엣지: 아우터를 입는 날에는 색이 보이도록 색이 있는 아우터를 쓰는 후보를 넣으세요 — 여민 코트 안의 상의 색은 거의 보이지 않습니다. 색이 있는 아우터나 상의에는 role 이 베이스·연결템인 담백한 하의와 가방을 받쳐 주세요.
";

/// 팝의 원칙 3~5. 기본 원칙의 "강한 포인트 1개 이하·중립적인 이너·안정적인 하의" 는 서로 다른
/// 색 옷의 조합과 그래픽 옷을 처음부터 밀어낸다. 그렇다고 모든 옷을 강한 색·무늬로 덮으라는
/// 공식도 두지 않는다 — 세 방향(색 조합·배색/그래픽 옷·배색 아우터)이 모두 팝이다.
const POP_PRINCIPLES: &str = "3. 각 후보는 색이나 그래픽이 착장의 중심이 되게 하세요. 방향은 셋입니다 — 서로 다른 색의 옷을 함께 입는 색 조합, 색 있는 배색·그래픽·대담한 패턴 옷이 중심인 조합, 배색·컬러 아우터가 중심인 조합. 두 컬러 옷이 함께 어울리는 것도 괜찮습니다.
4. 색이나 무늬가 중심이 아닌 옷은 형태가 단순한 옷으로 받치세요. 모든 옷을 강한 색·무늬로 덮을 필요는 없습니다.
5. 가방·신발은 코디를 거들 뿐 중심이 되지 못합니다. 기본 상의·하의에 색 있는 가방·신발만 더한 조합, 선명한 옷 한 벌에 무채색만 맞춘 조합은 팝 후보로 내지 마세요.";

/// 팝 후보의 방향. 목록의 팝 옷(배색·그래픽 옷, 선명한 색 옷)을 이름으로 알려 준다 — 프레피
/// 실측에서 "목록에 색이 있으면" 이라는 일반 문장은 모델이 색 옷을 고르게 하지 못했다.
fn pop_tasks(pop_items: &[String], temperature: f64) -> String {
    let mut out = String::from(
        "- 팝: 날씨와 후보 목록이 허락하면 5개 중 색 조합 후보, 배색·그래픽 옷이 중심인 후보, 배색·컬러 아우터가 중심인 후보를 각각 1개 이상 넣으세요. 그런 옷이 목록에 없으면 억지로 만들지 마세요.\n- 팝: 색 조합은 후보마다 다른 색 계열(괄호 안)을 쓰고, 같은 색 옷을 여러 후보에 반복하지 마세요.\n- 팝: 각 후보의 \"focus\" 에 색이나 그래픽이 중심인 옷의 슬롯(상의·하의·아우터 중 하나)을 적으세요.\n",
    );
    if !pop_items.is_empty() {
        out.push_str(&format!(
            "- 팝: 이번 목록의 팝 옷은 {} 입니다.\n",
            pop_items.join(", ")
        ));
    }
    if temperature < GLAM_COVER_COAT_BELOW_C {
        out.push_str("- 팝(추운 날): 코트는 소매에 팔을 넣고 여며 입는다고 보고 고르세요. 여민 코트 속 상의의 색·무늬는 보이지 않으니 배색·컬러 아우터나 색 있는 하의로 팝을 만드세요. 5°C 아래에서는 보온 안감(insulated lining, down fill 등)이 적힌 두꺼운 아우터를 고르세요.\n");
    }
    out
}

/// 추운 날 엣지. 보온 판정(`serving_ranker`)과 같은 기준을 미리 알려, 안감 없는 라이더·
/// 레더 블레이저나 맨다리 미니가 후보로 나왔다가 날씨 게이트에서 빠지는 일을 줄인다.
const EDGY_COLD_TASK: &str = "- 엣지(추운 날): 오늘 밖에서 입고 다니는 착장입니다. 코트는 소매에 팔을 넣고 여며 입는다고 보고 고르세요. 5°C 아래에서는 보온 안감(insulated lining, shearling lining 등)이 적힌 두꺼운 아우터를 고르고, 안감이 적히지 않은 라이더·레더 블레이저·크롭 재킷에 기대지 마세요.
- 엣지(추운 날): 미니처럼 짧은 하의는 니하이 부츠와 무릎을 덮는 롱코트가 함께일 때만 고르세요. 그렇지 않으면 미디 스커트나 긴 바지로 하의를 정하세요. 목록에 없는 스타킹·타이츠를 더하지 마세요.
";

/// 이 기온 미만의 글램은 "실내 착장 + 이동용 두꺼운 코트" 로 짠다.
pub const GLAM_COVER_COAT_BELOW_C: f64 = 12.0;

/// 추운 날 글램의 장면. 한겨울에 글램 착장을 아우터까지 맞춰 입고 걷지는 않는다 — 실내에서
/// 보여 줄 착장 위에 두꺼운 코트를 걸치고 택시로 이동한다(2026-10-17 사용자). 짧은 하의와
/// 맨다리도 이 장면에서는 자연스럽다.
const GLAM_COLD_TASK: &str = "- 글램(추운 날): 오늘 밖에서 입고 다니는 착장입니다. 코트는 소매에 팔을 넣고 여며 입는다고 보고 고르세요. [아우터 후보] 중 두께가 thick 인 글램 코트(퍼·시어링·아워글라스 실루엣·선명한 색)를 넣고, 단정한 무광 코트로 대신하지 마세요. 5°C 아래에서는 보온 안감(insulated lining, shearling lining 등)이 적힌 아우터를 고르고, 안감이 적히지 않은 라이더·크롭 재킷에 기대지 마세요.
- 글램(추운 날): 미니처럼 짧은 하의는 니하이 부츠와 무릎을 덮는 롱코트가 함께일 때만 고르세요. 그렇지 않으면 롱·미디 스커트나 팬츠로 하의를 정하세요. 목록에 없는 스타킹·타이츠를 더하지 마세요.
- 글램(추운 날): 코트가 중심이면 안의 옷은 슬림한 상의·스트레이트 팬츠처럼 담백하게, 상의가 중심이면 코트는 몸의 선과 비율로 받쳐 주는 코트로 고르세요. 퍼·가죽·새틴을 한 착장에서 한꺼번에 강조하지 말고 착장마다 중심 소재를 하나 정하세요. 후보마다 같은 이너를 반복하지 마세요.
";

/// 슬롯별 후보 목록에서 5가지 코디 초안을 받는다.
///
/// 아우터 규칙을 온도로 못박고 응답 예시에도 아우터를 넣어 둔 이유: 예시에 아우터 줄이
/// 없고 "필요할 때만" 이라고만 적혀 있던 동안, 모델은 15°C 에서도 5개 후보 전부를
/// 아우터 없이 채웠다. 그러면 블레이저·코트 같은 아우터는 shortlist 까지 올라와도
/// 후보에 한 번도 등장하지 못해 규칙 엔진이 비교할 기회조차 없다.
#[allow(clippy::too_many_arguments)]
pub async fn get_outfit_candidates(
    llm: &LlmClient,
    weather: &CurrentWeather,
    clothes: &GroupedClothes,
    occasion: Option<&str>,
    style_preference: Option<&str>,
    recent_items_hint: &str,
    mood: Option<crate::models::style_vocab::StyleGenre>,
    colour_variety: bool,
    palette: bool,
) -> anyhow::Result<AiMultiRecommendation> {
    let today = chrono::Local::now().format("%Y-%m-%d (%A)").to_string();
    let (system_prompt, user_prompt) = outfit_candidate_prompts(
        weather,
        clothes,
        occasion,
        style_preference,
        recent_items_hint,
        &today,
        mood,
        colour_variety,
        palette,
    );
    Ok(llm
        .chat_json::<AiMultiRecommendation>(
            LlmTask::OutfitCandidates,
            ChatRequest::new(vec![Message::user_text(user_prompt)])
                .system(&system_prompt)
                .json(),
        )
        .await?)
}

/// 후보 생성 프롬프트(시스템, 사용자). 호출과 떼어 두어 문장을 테스트로 확인한다.
#[allow(clippy::too_many_arguments)]
pub fn outfit_candidate_prompts(
    weather: &CurrentWeather,
    clothes: &GroupedClothes,
    occasion: Option<&str>,
    style_preference: Option<&str>,
    recent_items_hint: &str,
    today: &str,
    mood: Option<crate::models::style_vocab::StyleGenre>,
    // 엣지에서 색 다양성 문장을 넣을지. 밝은색 비선호 사용자면 `false` 다.
    colour_variety: bool,
    // 색 다양성 성별·무드(`mood_palette::targets`)인가. 밝은색 비선호 사용자면 `false` 다.
    palette: bool,
) -> (String, String) {
    // 글램은 기본 원칙 3~5(강한 포인트 1개 이하·안정적인 하의·중립적인 이너)를 글램의
    // 원칙으로 바꾼다. 두 원칙을 함께 두면 모델이 한쪽을 무시한다. 다른 무드의 문장은
    // 예전과 같다.
    let glam = mood == Some(crate::models::style_vocab::StyleGenre::Glam);
    let edgy = mood == Some(crate::models::style_vocab::StyleGenre::Edgy);
    let pop = mood == Some(crate::models::style_vocab::StyleGenre::Pop);
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

    let palette_principle = mood.filter(|_| palette).and_then(palette_principles);
    let principles = if glam {
        GLAM_PRINCIPLES
    } else if edgy {
        EDGY_PRINCIPLES
    } else if pop {
        POP_PRINCIPLES
    } else if let Some(p) = palette_principle {
        p
    } else {
        DEFAULT_PRINCIPLES
    };
    let system_prompt = format!(
        r#"당신은 다양한 현대 남녀 패션 스타일과 의류 구성에 익숙한 코디 보조 AI입니다.

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
{principles}
6. 같은 색/무드로 몰리지 마세요.

5가지 서로 다른 후보 코디를 제안하세요.
반드시 JSON 형식으로 응답하세요."#
    );

    let user_prompt = format!(
        r#"오늘 날짜: {today}

현재 조건:
- 기온: {temp}°C (체감 {feels}°C)
- 습도: {humidity}%
- 바람: {wind} km/h
- 날씨: {desc}
- 상황: {occasion}
- 선호 스타일: {style}{mood_line}
{recent_section}
{clothes}

작업:
- 위 슬롯별 후보 목록에서만 5가지 서로 다른 코디 후보를 구성하세요.
- 각 후보는 다른 아이템 조합이어야 합니다.
- 상의와 하의 중 최소 하나는 후보마다 달라야 합니다.
- 기온이 20°C 미만이면 아우터를 넣은 후보와 넣지 않은 후보를 **둘 다** 포함하세요.
  어느 쪽이 나은지는 규칙 엔진이 고릅니다. 한쪽으로만 5개를 채우지 마세요.
{mood_tasks}- 신발은 반드시 [신발 후보]에서만 고르세요.
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
{focus_field}      "weather_summary": "날씨 요약 한 줄",
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
        mood_line = if glam {
            "\n- 선택한 무드: 글램 — 빛나는 소재와 선명한 실루엣으로 존재감을 주는 스타일. \
             저녁 약속·파티·분위기 있는 장소에서 시선을 끄는 착장을 만듭니다."
        } else if edgy {
            "\n- 선택한 무드: 엣지 — 비대칭과 구조적인 실루엣, 가죽과 니트·데님·직물의 대비로 \
             취향과 존재감을 드러내는 도시적인 스타일. 일상 외출에서 시선을 끄는 착장을 만듭니다."
        } else if pop {
            "\n- 선택한 무드: 팝 — 선명한 색의 조합, 컬러 그래픽·기하 패턴, 배색 아우터로 유쾌하게 \
             시선을 끄는 성인 여성의 일상복. 코스튬·아이 같은 연출이 아니라 실제로 입는 착장입니다."
        } else if palette_principle.is_some() {
            match mood {
                Some(crate::models::style_vocab::StyleGenre::Street) => "\n- 선택한 무드: 스트리트",
                Some(crate::models::style_vocab::StyleGenre::SportyCasual) => {
                    "\n- 선택한 무드: 스포티 캐주얼"
                }
                Some(crate::models::style_vocab::StyleGenre::Amekaji) => {
                    "\n- 선택한 무드: 아메카지"
                }
                _ => "\n- 선택한 무드: 프레피",
            }
        } else {
            ""
        },
        mood_tasks = if glam {
            if weather.temperature < GLAM_COVER_COAT_BELOW_C {
                format!("{GLAM_TASKS}{GLAM_COLD_TASK}")
            } else {
                GLAM_TASKS.to_string()
            }
        } else if edgy {
            let colour = if colour_variety {
                EDGY_COLOUR_TASKS
            } else {
                ""
            };
            if weather.temperature < GLAM_COVER_COAT_BELOW_C {
                format!("{EDGY_TASKS}{colour}{EDGY_COLD_TASK}")
            } else {
                format!("{EDGY_TASKS}{colour}")
            }
        } else if pop {
            pop_tasks(&clothes.colour_items, weather.temperature)
        } else if palette_principle.is_some() {
            palette_tasks(mood, &clothes.colour_items)
        } else {
            String::new()
        },
        focus_field = if glam || edgy || pop {
            "      \"focus\": \"상의|하의|아우터 중 시선이 모이는 곳\",\n"
        } else {
            ""
        },
        recent_section = if recent_items_hint.is_empty() {
            String::new()
        } else {
            format!(
                "\n최근 추천된 아이템 (가능하면 다른 조합을 시도하세요):\n{}\n",
                recent_items_hint
            )
        },
    );

    (system_prompt, user_prompt)
}

// ─── 2) analyze_clothing_image (fallback, no RAG) ───

/// 옷장의 카테고리 규칙. 세 Vision 프롬프트(일반 분석, 1차, 2차)가 같은 문장을 쓴다.
///
/// 옷장 데이터는 원피스를 `하의`, 가디건·후드집업을 `아우터` 로 둔다. 프롬프트에 이
/// 규칙이 없을 때 실사진 25장 평가에서 원피스 3장과 가디건·볼레로가 `상의` 로 나왔다.
/// 그러면 저장된 분류가 옷장 관례와 어긋나고, 1차 분석의 카테고리로 레퍼런스를 좁히는
/// 검색도 원피스를 상의 레퍼런스(스웻셔츠, 캐미솔)와 비교한다.
///
/// 매크로인 이유: 리터럴이어야 `concat!` 과 `format!` 의 형식 문자열에 함께 넣을 수 있다.
///
/// 넣었다가 뺀 문장: "앞이 위에서 아래까지 단추로 열리는 니트는 잠가 입었어도 가디건".
/// 단추를 잠가 입은 가디건 사진이 `상의` 로 나와서 시험했지만, 같은 25장을 3회씩 돌린
/// 결과 그 사진은 3회 모두 그대로 `상의` 였다. 볼레로는 안정됐으나 다른 자켓 이름이
/// 덜 구체적이 되어 득실이 없었다. 이런 사진은 등록 직후 확인 화면에서 사용자가
/// 종류를 고치게 한다(`WardrobePage` 의 `GenreConfirm`).
macro_rules! category_rule {
    () => {
        "카테고리는 이 서비스의 옷장 규칙을 따릅니다. 원피스는 '하의' 입니다. \
가디건, 볼레로, 후드집업처럼 다른 옷 위에 걸쳐 입는 앞트임 옷은 '아우터' 입니다."
    };
}

/// `is_clothing` 의 뜻. 이름과 달리 "옷인가" 가 아니라 "옷장에 넣을 패션 아이템인가" 다.
///
/// 이 설명이 없을 때 가방 사진에 `is_clothing: false` 가 3번 모두 돌아왔다 — 가방은 옷이
/// 아니라고 읽은 것이다. 카테고리와 속성은 다 채우고 이름만 null 로 비웠고, 등록 경로는
/// false 를 보고 "의류 항목이 아닙니다" 로 거절했다.
macro_rules! fashion_item_rule {
    () => {
        "is_clothing 은 이 사진이 옷장에 넣을 패션 아이템인지입니다. 신발, 가방, 모자, \
벨트, 액세서리도 true 입니다. 음식, 풍경, 사람 얼굴만 찍힌 사진처럼 패션 아이템이 \
아닐 때만 false 로 두고 rejection_reason 을 쓰세요."
    };
}

/// texture_worlds 로 고를 수 있는 값.
///
/// romantic·bohemian 은 2026-10-11 에 생겼다 — 그 전에는 레이스 블라우스나 크로셰
/// 가디건이 들어갈 월드가 없어 비어 있었다. 두 프롬프트(일반 분석, RAG 2차)가 같은
/// 목록을 쓰도록 한 곳에 둔다.
///
/// 셔링 설명은 생김새로 적는다. 이름만 적었을 때 셔링 탑 2장이 "버튼 장식 긴팔 탑" 으로
/// 읽혀 minimal 이 됐다(2026-10-11, 실사진 12장).
macro_rules! texture_world_rule {
    () => {
        "- texture_worlds는 workwear, military, tailoring, sweat, outdoor, minimal, romantic, bohemian 중 복수 선택 가능.\n  romantic: 레이스·셔링·스모킹·퍼프 소매·리본·새틴·쉬폰·오간자처럼 여성스러운 장식이나 광택이 있는 옷.\n  셔링은 원단을 잡아당겨 생긴 잔주름이다 — 앞판·옆선·소매에 오그라든 주름이 모여 있으면(단추가 함께 달려 있어도) 셔링이다.\n  오프숄더·카울넥처럼 어깨를 드러내거나 목선이 흘러내리는 탑도 romantic 이다.\n  bohemian: 크로셰·에스닉 자수·티어드 맥시·구김 면처럼 자연스럽고 느슨한 옷.\n"
    };
}

/// 브랜드 규칙. 2026-10-11 결정: 로고나 브랜드 표기가 사진에 보일 때만 브랜드를 붙인다.
///
/// 예전 문장("확실히 식별할 수 없으면 추측하지 말 것")은 무엇이 "확실" 한지 정하지 않아,
/// 생김새가 유명한 제품이면 로고 없이도 브랜드를 붙일 여지가 있었다. 기준을 사진 속
/// 근거로 못박는다. 레퍼런스 설명에 브랜드가 적혀 있어도 그것은 근거가 아니다.
///
/// 표기는 영문 원어 그대로다(2026-10-11 결정). 같은 제품이 "Nike Air Jordan 1" 과
/// "에어 조던 1" 로 번갈아 나왔다.
///
/// MA-1 · M-65 같은 군용 규격명은 브랜드 모델명이 아니라 여러 브랜드가 만드는 아이템
/// 이름이다. 레퍼런스 검색이 알아내려는 것이 바로 이것이라, 로고 조건에서 따로 뺀다.
macro_rules! brand_rule {
    () => {
        "브랜드는 로고나 브랜드 표기(글자, 라벨, 태그)가 사진에 보일 때만 이름에 넣으세요. \
로고 없이 생김새만 보고, 또는 참고자료에 적힌 브랜드를 보고 브랜드를 붙이면 안 됩니다. \
모델명은 브랜드 로고가 보이고 그 모델의 고유한 디자인이 분명할 때만 넣으세요. \
브랜드와 모델명은 한글로 옮기지 말고 원래 영문 표기 그대로 쓰세요 \
(예: \"블루/화이트 Nike Air Jordan 1 하이탑 스니커\", \"나이키 에어 조던\" 은 안 됩니다). \
MA-1, M-65, 트렌치코트처럼 여러 브랜드가 만드는 아이템 이름은 브랜드가 아니므로, \
형태가 맞으면 로고 없이도 쓸 수 있습니다."
    };
}

/// 이름 규칙. 빈 문자열도 null 과 같이 금지한다.
///
/// "null 금지" 만 적었을 때 신발·가방 사진 3장에서 빈 문자열이 돌아왔다. 브랜드를
/// 확신하지 못하면 이름을 아예 비우는 것으로 보여, 그때 쓸 형식을 함께 준다.
macro_rules! name_rule {
    () => {
        "name 은 null 도 빈 문자열도 안 됩니다. 브랜드나 모델명을 모르면 \
\"색상 + 생김새 + 아이템명\" 으로 쓰세요 (예: \"베이지 웨스턴 부츠\", \"스트라이프 패브릭 숄더백\")."
    };
}

/// 레퍼런스 없는 분석의 시스템 프롬프트. 평가 도구가 같은 문장으로 원문 응답을
/// 받아 볼 수 있도록 공개한다.
pub const GENERAL_ANALYSIS_PROMPT: &str = concat!(
    r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 분석 AI입니다.
사용자가 업로드한 이미지에서 의류/신발/가방/모자/벨트 등 패션 아이템을 분석하여 구조화된 정보를 추출하세요.

가장 중요한 원칙:
1. 보이는 것만 바탕으로 판단하세요.
2. "#,
    brand_rule!(),
    r#"
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
- "#,
    name_rule!(),
    r#"
- gender 는 이 옷이 주로 어느 쪽 옷장에 들어갈지입니다. 어느 쪽이든 입을 수 있으면 unisex.
- style_genres 는 이 옷이 어울리는 장르를 **모두** 고릅니다. 한 벌이 여러 장르에
  들어갈 수 있습니다 — 옥스퍼드 셔츠는 classic 이면서 amekaji 이고 preppy 입니다.
  고를 수 있는 값: minimal(장식 없는 단순한 실루엣, 적은 색상),
  classic(셔츠·재킷·트렌치의 단정한 핏), romantic(부드러운 소재와 곡선),
  modern_chic(선명한 실루엣과 강한 대비), bohemian(자연 소재와 느슨한 레이어링),
  street(오버사이즈와 그래픽, 데님·카고), mannish(테일러링과 넓은 어깨),
  sporty_casual(기능성 소재와 운동복 요소), glam(새틴·메탈릭·벨벳처럼 빛을 받는 소재나
  원숄더·드레이프 같은 조형적인 실루엣), edgy(비대칭·각진 어깨·배럴 레그·박시 크롭처럼
  형태가 분명한 옷이나 가죽 옷 — 검은색이나 오버사이즈라는 것만으로는 고르지 않습니다),
  pop(컬러블록·컬러 그래픽·대담한 컬러 패턴처럼 색과 무늬가 중심인 옷, 또는 코발트·체리레드·
  켈리그린처럼 채도가 높아 다른 선명한 색과 섞어 입을 단색 옷 — 흐린 색·파스텔·네이비 같은
  기본색이나 흑백 그래픽이라는 것만으로는 고르지 않습니다),
  amekaji(아메리칸 캐주얼),
  preppy(아이비리그), workwear(작업복), outdoor_casual(기능성 아웃도어).
  확신이 서는 것만 고르고, 애매하면 비워 두세요 — 틀린 장르에 들어가면 그 장르의
  추천이 전부 어긋납니다.
- "#,
    fashion_item_rule!(),
    r#"
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요.
"#,
    texture_world_rule!(),
    r#"- category는 상의, 하의, 아우터, 신발, 액세서리, 가방, 모자, 벨트 중 하나입니다. "#,
    category_rule!()
);

pub async fn analyze_clothing_image(
    llm: &LlmClient,
    image_data_url: &str,
) -> anyhow::Result<VisionAnalysisResult> {
    let system_prompt = GENERAL_ANALYSIS_PROMPT;

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
    let system_prompt = concat!(
        r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 감정사 AI입니다.
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
고르세요. 애매하면 가장 가까운 것을 고르고, 목록에 없으면 빈 문자열로 두세요.
"#,
        category_rule!()
    );

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
        concat!(
            r#"당신은 캐주얼, 클래식, 스트리트, 워크웨어, 아웃도어 및 스포츠웨어에 익숙한 의류 감정사 AI입니다.

## 후보 레퍼런스 (유사도 순)
{ref_context}

중요 원칙:
1. 이미지를 먼저 직접 관찰하세요.
2. 레퍼런스는 참고자료이지 정답이 아닙니다.
3. 레퍼런스와 이미지가 핵심 식별 특징(실루엣, 포켓 구조, 여밈 방식, 소재감)에서 충분히 일치할 때만 해당 모델명을 사용하세요.
4. 부분적으로만 유사하면 레퍼런스를 참고하되, 일반화된 구체명으로 작성하세요.
5. "#,
            brand_rule!(),
            r#"

판별 절차:
1. 아이템 종류를 먼저 판단
2. 같은 종류 레퍼런스만 비교
3. 핵심 특징이 강하게 일치하면 모델명 사용
4. 그렇지 않으면 이미지 관찰 기반 일반화된 구체명 사용

name 작성 원칙:
- 가장 우선은 정확성
- 형식: "색상 + 소재/스타일 + 아이템명"
- 확실할 때만 브랜드/모델명 포함
- "#,
            name_rule!(),
            r#"

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
  sporty_casual(기능성 소재와 운동복 요소), glam(새틴·메탈릭·벨벳처럼 빛을 받는 소재나
  원숄더·드레이프 같은 조형적인 실루엣), edgy(비대칭·각진 어깨·배럴 레그·박시 크롭처럼
  형태가 분명한 옷이나 가죽 옷 — 검은색이나 오버사이즈라는 것만으로는 고르지 않습니다),
  pop(컬러블록·컬러 그래픽·대담한 컬러 패턴처럼 색과 무늬가 중심인 옷, 또는 코발트·체리레드·
  켈리그린처럼 채도가 높아 다른 선명한 색과 섞어 입을 단색 옷 — 흐린 색·파스텔·네이비 같은
  기본색이나 흑백 그래픽이라는 것만으로는 고르지 않습니다),
  amekaji(아메리칸 캐주얼),
  preppy(아이비리그), workwear(작업복), outdoor_casual(기능성 아웃도어).
  확신이 서는 것만 고르고, 애매하면 비워 두세요 — 틀린 장르에 들어가면 그 장르의
  추천이 전부 어긋납니다.
- "#,
            fashion_item_rule!(),
            r#"
- is_clothing이 false이면 name/category/color/thickness/seasons는 null, rejection_reason을 작성하세요.
"#,
            texture_world_rule!(),
            "- ",
            category_rule!()
        ),
        // concat! 으로 만든 형식 문자열은 변수를 이름으로 자동 캡처하지 못해 명시한다.
        ref_context = ref_context,
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

#[cfg(test)]
mod outfit_candidate_prompt_tests {
    use super::*;
    use crate::models::style_vocab::StyleGenre;

    fn prompts(mood: Option<StyleGenre>) -> (String, String) {
        prompts_at(mood, 17.0)
    }

    fn prompts_at(mood: Option<StyleGenre>, temperature: f64) -> (String, String) {
        prompts_with(mood, temperature, true)
    }

    fn prompts_with(
        mood: Option<StyleGenre>,
        temperature: f64,
        colour_variety: bool,
    ) -> (String, String) {
        prompts_full(mood, temperature, colour_variety, false, &[])
    }

    fn prompts_full(
        mood: Option<StyleGenre>,
        temperature: f64,
        colour_variety: bool,
        palette: bool,
        colour_items: &[String],
    ) -> (String, String) {
        let w = CurrentWeather {
            temperature,
            apparent_temperature: 16.0,
            humidity: 50.0,
            wind_speed: 3.0,
            weather_code: 0,
            weather_description: "맑음".into(),
        };
        let g = GroupedClothes {
            tops: vec!["- 티 | role:베이스 | tone:밝음 | style:베이직".into()],
            bottoms: vec!["- 데님".into()],
            outers: vec![],
            shoes: vec!["- 스니커".into()],
            bags: vec![],
            colour_items: colour_items.to_vec(),
        };
        outfit_candidate_prompts(
            &w,
            &g,
            Some("일상"),
            None,
            "- 티",
            "2026-10-17 (Saturday)",
            mood,
            colour_variety,
            palette,
        )
    }

    /// 글램·엣지·팝이 아닌 무드는 예전 문장 그대로다 — 무드 전달이 다른 무드의 후보를 바꾸지
    /// 않는다. 추운 날 문장도 다른 무드에는 들어가지 않는다.
    #[test]
    fn other_moods_get_the_unchanged_prompt() {
        for t in [17.0, 2.0] {
            let base = prompts_at(None, t);
            for g in StyleGenre::ALL
                .iter()
                .filter(|g| !matches!(g, StyleGenre::Glam | StyleGenre::Edgy | StyleGenre::Pop))
            {
                assert_eq!(prompts_at(Some(*g), t), base, "{g}");
            }
            assert!(base.0.contains(DEFAULT_PRINCIPLES));
            for word in ["글램", "엣지", "팝 "] {
                assert!(!base.0.contains(word) && !base.1.contains(word));
            }
        }
    }

    /// 엣지는 기본 원칙(강한 포인트 1개 이하·안정적인 하의·중립적인 이너)을 엣지 원칙으로
    /// 바꾼다. 글램 문장은 섞이지 않는다. "중심은 한 곳" 같은 공식도 두지 않는다.
    #[test]
    fn edgy_swaps_the_conflicting_principles() {
        let (sys, user) = prompts(Some(StyleGenre::Edgy));
        assert!(sys.contains(EDGY_PRINCIPLES));
        assert!(!sys.contains(DEFAULT_PRINCIPLES));
        assert!(!sys.contains(GLAM_PRINCIPLES));
        assert!(user.contains("선택한 무드: 엣지"));
        assert!(user.contains(EDGY_TASKS));
        assert!(!user.contains("글램"));
        assert!(user.contains("\"focus\""));
        assert!(!user.contains(EDGY_COLD_TASK));
        assert!(!sys.contains("한 곳에 시선이 모이는 중심"));
    }

    /// 팝은 기본 원칙 3~5를 팝 원칙으로 바꾸고, 세 방향과 목록의 팝 옷을 이름으로 알린다.
    /// 추운 날에는 여민 코트 속 상의 색이 보이지 않는다는 문장이 붙는다. 팝은 사용자가 직접
    /// 고른 무드라 밝은색 비선호 값과 무관하게 같은 문장이다.
    #[test]
    fn pop_swaps_principles_and_names_its_pieces() {
        let items = vec![
            "레드 핑크 컬러블록 스웨트셔츠(배색·그래픽)".to_string(),
            "코발트 와이드 팬츠(블루)".to_string(),
        ];
        for t in [19.6, 8.0, 2.0] {
            let (sys, user) = prompts_full(Some(StyleGenre::Pop), t, true, false, &items);
            assert!(sys.contains(POP_PRINCIPLES));
            assert!(!sys.contains(DEFAULT_PRINCIPLES));
            assert!(!sys.contains(EDGY_PRINCIPLES) && !sys.contains(GLAM_PRINCIPLES));
            assert!(user.contains("선택한 무드: 팝"));
            assert!(user.contains("색 조합 후보, 배색·그래픽 옷이 중심인 후보, 배색·컬러 아우터"));
            assert!(user.contains("다른 색 계열"));
            assert!(user.contains(
                "이번 목록의 팝 옷은 레드 핑크 컬러블록 스웨트셔츠(배색·그래픽), 코발트 와이드 팬츠(블루) 입니다."
            ));
            assert!(user.contains("\"focus\""));
            assert_eq!(user.contains("팝(추운 날)"), t < 12.0);
            assert!(!user.contains("글램") && !user.contains("엣지"));
            assert_eq!(
                prompts_full(Some(StyleGenre::Pop), t, false, false, &items),
                (sys, user)
            );
        }
        // 팝 옷이 없으면 이름 목록 문장이 없다.
        let (_, user) = prompts_full(Some(StyleGenre::Pop), 19.6, true, false, &[]);
        assert!(!user.contains("이번 목록의 팝 옷"));
    }

    /// 엣지의 색 다양성 문장은 기본으로 들어가고, 밝은색 비선호 사용자에게는 빠진다. 형태·
    /// 추운 날 기준 문장은 둘 다 그대로다. 다른 무드에는 이 값이 아무 영향이 없다.
    #[test]
    fn edgy_colour_variety_follows_the_bright_colour_preference() {
        for t in [16.0, 2.0] {
            let (_, on) = prompts_with(Some(StyleGenre::Edgy), t, true);
            let (_, off) = prompts_with(Some(StyleGenre::Edgy), t, false);
            assert!(on.contains(EDGY_COLOUR_TASKS));
            assert!(!off.contains(EDGY_COLOUR_TASKS));
            assert!(off.contains(EDGY_TASKS));
            assert_eq!(on.contains(EDGY_COLD_TASK), t < 12.0);
            assert_eq!(off.contains(EDGY_COLD_TASK), t < 12.0);
            for g in [None, Some(StyleGenre::Glam), Some(StyleGenre::Minimal)] {
                assert_eq!(prompts_with(g, t, true), prompts_with(g, t, false));
            }
        }
    }

    /// 색 다양성 무드(여성 스트리트·스포티, 남성 아메카지·프레피)는 기본 원칙 5("이너는
    /// 중립적")만 장르의 색 조합으로 바꾸고, 목록의 색 있는 옷을 이름으로 알린다. 색 있는 옷이
    /// 없으면 색 지시를 넣지 않는다. 꺼져 있으면(밝은색 비선호) 예전 문장 그대로다.
    #[test]
    fn palette_moods_swap_only_the_neutral_inner_principle() {
        let items = vec!["라이트블루 옥스포드 셔츠(블루)".to_string()];
        for g in [
            StyleGenre::Street,
            StyleGenre::SportyCasual,
            StyleGenre::Amekaji,
            StyleGenre::Preppy,
        ] {
            let (sys, user) = prompts_full(Some(g), 19.6, true, true, &items);
            assert!(
                !sys.contains("5. 이너는 중립적이고 활용도 높은 아이템 우선."),
                "{g}"
            );
            assert!(
                sys.contains("3. 강한 포인트 아이템은 1개 이하로 유지하세요."),
                "{g}"
            );
            assert!(sys.contains("색 없는 조합도"), "{g}");
            assert!(user.contains("라이트블루 옥스포드 셔츠(블루)"), "{g}");
            assert!(user.contains("선택한 무드"), "{g}");
            let (_, empty) = prompts_full(Some(g), 19.6, true, true, &[]);
            assert!(!empty.contains("- 색:"), "{g}");
            assert_eq!(
                prompts_full(Some(g), 19.6, true, false, &items),
                prompts_full(None, 19.6, true, false, &[]),
                "{g}"
            );
        }
    }

    /// 아메카지만, 목록에 색 계열이 둘 이상일 때 색 후보끼리 다른 계열을 쓰라고 한다. 계열이
    /// 하나뿐이거나 다른 대상 무드면 이 문장이 없다.
    #[test]
    fn amekaji_asks_for_different_colour_families() {
        let two = vec![
            "러스트 셔츠(오렌지)".to_string(),
            "블루 셔츠(블루)".to_string(),
        ];
        let one = vec![
            "러스트 셔츠(오렌지)".to_string(),
            "러스트 재킷(오렌지)".to_string(),
        ];
        let has = |g, items: &[String]| {
            prompts_full(Some(g), 19.6, true, true, items)
                .1
                .contains("서로 다른 색 계열")
        };
        assert!(has(StyleGenre::Amekaji, &two));
        assert!(!has(StyleGenre::Amekaji, &one));
        for g in [
            StyleGenre::Preppy,
            StyleGenre::Street,
            StyleGenre::SportyCasual,
        ] {
            assert!(!has(g, &two), "{g}");
        }
    }

    /// 다른 무드는 색 다양성 값과 무관하게 예전 문장이다. 엣지·글램도 그대로다.
    #[test]
    fn palette_flag_does_not_touch_other_moods() {
        let items = vec!["라벤더 니트(퍼플)".to_string()];
        for g in [
            StyleGenre::Romantic,
            StyleGenre::Minimal,
            StyleGenre::Bohemian,
            StyleGenre::Edgy,
            StyleGenre::Glam,
        ] {
            assert_eq!(
                prompts_full(Some(g), 19.6, true, true, &items),
                prompts_full(Some(g), 19.6, true, false, &items),
                "{g}"
            );
        }
    }

    /// 추운 날 엣지는 보온 안감·하체 기준을 미리 알린다. 날씨 판정과 같은 기준이다.
    #[test]
    fn cold_edgy_asks_for_lined_outerwear_and_covered_legs() {
        let (_, user) = prompts_at(Some(StyleGenre::Edgy), 2.0);
        assert!(user.contains(EDGY_COLD_TASK));
        assert!(user.contains("보온 안감"));
        assert!(user.contains("니하이 부츠와 무릎을 덮는 롱코트"));
        assert!(!user.contains(GLAM_COLD_TASK));
    }

    /// 글램은 기본 원칙(강한 포인트 1개 이하·중립적인 이너)을 글램 원칙으로 바꾼다 — 두
    /// 원칙을 함께 두지 않는다. 무드와 방향 요청, focus 칸이 들어간다.
    #[test]
    fn glam_swaps_the_conflicting_principles() {
        let (sys, user) = prompts(Some(StyleGenre::Glam));
        assert!(sys.contains(GLAM_PRINCIPLES));
        assert!(!sys.contains("5. 이너는 중립적이고 활용도 높은 아이템 우선."));
        assert!(!sys.contains("3. 강한 포인트 아이템은 1개 이하로 유지하세요."));
        assert!(user.contains("선택한 무드: 글램"));
        assert!(user.contains("상의가 중심인 후보, 하의가 중심인 후보, 아우터가 중심인 후보"));
        assert!(user.contains("\"focus\""));
    }
}
