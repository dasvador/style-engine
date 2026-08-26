# Style Engine

[![CI](https://github.com/dasvador/style-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/dasvador/style-engine/actions/workflows/ci.yml)

**옷 사진을 올려 옷장을 만들고, 그 옷들로 짠 코디를 점수와 이유로 평가받는 서비스입니다.**

옷 사진을 업로드하면 Vision 모델이 무슨 아이템인지 알아내고 색·무게감·격식 같은 11개 속성을
자동으로 붙여 옷장에 저장합니다. 저장된 옷 중 상의/하의/아우터/신발/가방을 골라 "출근" 같은
상황을 지정하면, 규칙 엔진이 그 조합을 0~95점으로 채점하고 **무엇이 왜 문제인지, 무엇으로
바꾸면 되는지**를 구조화된 형태로 돌려줍니다.

핵심 설계는 **점수를 LLM에게 맡기지 않는 것**입니다. 판정은 결정론적 규칙 엔진이 내리고,
LLM은 이미 정해진 결론을 자연어로 풀어 쓰기만 합니다. 덕분에 같은 코디는 언제나 같은 점수를
받고, 엔진 품질을 96건의 라벨링된 케이스로 회귀 테스트할 수 있습니다.

## 실제 동작 예시

```jsonc
// POST /api/outfit/evaluate
{
  "top":    "웜그레이 크루넥 티셔츠",   // 실제로는 clothing id
  "bottom": "샌드 와이드 팬츠",
  "outer":  "샌드 코튼 자켓",
  "shoes":  "샌드 캔버스 슬립온",
  "situation": "출근"
}
```

```jsonc
{
  "score": 35,
  "verdict": "Awkward",
  "verdict_label": "아쉬워요",
  "summary": "샌드 코튼 자켓 샌드 와이드 팬츠와의 조합에서 — 상황과 격식 수준이 맞지 않습니다.",

  "problems": [
    { "code": "TooMuchNaturalTone", "deduction": 18,
      "detail": "모든 아이템이 웜톤이고 중심을 잡아줄 구조가 없어 상당히 흐려 보입니다" },
    { "code": "FormalitySituationMismatch", "deduction": 20,
      "detail": "출근에 비해 코디가 너무 캐주얼합니다 (평균 격식도: 1.5)" },
    { "code": "SeasonalMismatch", "deduction": 15,
      "detail": "대부분의 아이템이 현재 계절(여름)에 맞지 않습니다: ..." }
    // LackOfStructure -7 생략
  ],

  // 문제마다 "무엇으로 바꿀지"까지 구조화해서 제시 — UI가 그대로 필터로 쓸 수 있다
  "suggestions": [
    { "type": "upgrade_formality", "reason_code": "FormalitySituationMismatch",
      "reason": "상황에 비해 너무 캐주얼합니다",
      "recommended_roles":  ["구조템"],
      "recommended_colors": ["네이비", "차콜"],
      "recommended_examples": ["옥스포드 셔츠", "테일러드 자켓"] }
  ],

  // 위 결론을 바꾸지 않고 자연어로 풀어 쓰는 것만이 LLM의 역할
  "explanation": "이번 코디는 웜그레이 크루넥 티셔츠와 샌드 팬츠로 안정적인 느낌을 주고 있습니다.
                  하지만 전체적으로 단조로운 색조와 구조감 부족으로 인해 시각적으로 흐릿해 보이는
                  점이 아쉽습니다. 출근에 적합한 격식이 부족하며, ..."
}
```

`score`·`verdict`·`problems`·`suggestions`는 전부 규칙 엔진이 만든 값이고, `explanation`만
LLM이 씁니다. LLM을 꺼도 이 응답은 `explanation`을 뺀 채로 그대로 동작합니다.

## 이 프로젝트에서 봐주셨으면 하는 것

세 가지를 의도적으로 만들었습니다.

| | 무엇 | 어디 |
|---|---|---|
| **1** | **비결정성 격리** — 판정은 규칙 엔진, LLM은 설명만. 그래서 테스트가 가능하다 | [`services/style_engine.rs`](src/services/style_engine.rs) |
| **2** | **Model Orchestration** — 호출부는 모델이 아니라 *task*를 지정한다. 모델 교체가 코드 수정이 아닌 설정 변경 | [`services/llm/`](src/services/llm/) |
| **3** | **Eval + 회귀 게이트** — 엔진 품질을 96건 케이스로 수치화하고, 기준선 대비 하락하면 CI가 실패한다 | [`tests/eval_scorecard.rs`](tests/eval_scorecard.rs) |
| **4** | **도메인 어휘를 타입으로** — 같은 버그를 두 번 낸 뒤, 잘못된 값이 들어올 수 있는 네 경계 전부에서 실패하게 만들었다 | [`models/style_vocab.rs`](src/models/style_vocab.rs) |

아래 [Model Orchestration](#model-orchestration-task-기반-provider-추상화)과
[품질 관리](#품질-관리-eval--ci)에서 각각 자세히 다룹니다.

## Tech Stack

| 구분 | 기술 |
|------|------|
| Language | Rust (Edition 2024) |
| Framework | Axum + Tokio |
| Database | MySQL (sqlx) |
| LLM | provider 추상화 레이어 (OpenAI / Anthropic) — task 단위 라우팅 |
| Vision | 기본값 `gpt-4o-mini` (설정으로 교체) |
| Embedding | 기본값 `text-embedding-3-small` (1536차원) |
| Weather | 기상청 초단기실황 API (Open-Meteo 폴백) |

---

# 아키텍처

## Model Orchestration: task 기반 provider 추상화

애플리케이션의 모든 모델 호출은 `services/llm` 한 곳을 지나갑니다. 호출부는 **모델이 아니라
task를 지정합니다** — "이 호출은 `VisionPass1`이다"라고만 말하고, 어떤 provider의 어떤 모델이
처리할지는 설정이 정합니다.

```
호출부 (routes/, services/prompts.rs)
    │  "LlmTask::VisionPass1로 이 요청을 처리해줘"
    ▼
LlmClient  ── task → provider/model 라우팅 (환경변수)
           ── 타임아웃 · 지수 백오프 재시도 · 응답 스키마 검증
           ── 토큰/지연/추정비용 계측 (구조화 로그 `llm_call`)
    ▼
ChatProvider / EmbeddingProvider / ImageProvider  (trait)
    ├── OpenAiProvider     chat · embedding · image
    └── AnthropicProvider  chat only
```

모델 교체가 코드 수정이 아니라 설정 변경이 됩니다:

```bash
LLM_TASK_VISION_PASS2=anthropic:claude-opus-5   # 이 task만 Anthropic으로
LLM_TASK_STYLE_NOTE=gpt-4o                      # 이 task만 상위 모델로
```

**능력 차이는 provider 구현체가 흡수합니다.** 호출부는 provider별 wire format을 모릅니다.

| 차이 | OpenAI | Anthropic | 레이어의 처리 |
|------|--------|-----------|--------------|
| system 프롬프트 | messages 배열 | 최상위 필드 | 각 구현체가 직렬화 |
| JSON 모드 | `response_format` 네이티브 | 스키마 없는 모드 없음 | 프롬프트 지시 + 코드펜스 제거 |
| 도구 인자 | 문자열 | 객체 | 중립 타입은 항상 객체 |
| 도구 결과 | 개별 `tool` 메시지 | user 메시지 하나로 묶어야 함 | 연속 결과를 자동 병합 |
| `temperature` | 지원 | 현행 모델은 400 거절 | Anthropic에는 전송 안 함 |
| thinking 토큰 | 없음 | `max_tokens` 잠식 | 여유분 자동 가산 |
| 거절(refusal) | 없음 | HTTP 200 + `stop_reason` | 상태코드 대신 본문 검사 |

이 표의 각 행은 양쪽 provider에 짝을 이루는 테스트로 고정되어 있습니다 — 같은 중립 요청이
각자의 형식으로 다르게 직렬화되는지가 이 레이어의 존재 이유이기 때문입니다.

`EmbeddingProvider`/`ImageProvider`를 `ChatProvider`와 분리한 이유: Anthropic은 임베딩·이미지
생성 엔드포인트가 없습니다. 하나의 거대한 trait이면 모든 구현체가 `unimplemented!()`을 들고
있게 되고, "이 provider로 교체 가능한가"를 판단할 수 없게 됩니다.

**재시도는 응답 스키마 검증까지 포함합니다.** 스키마에 맞지 않는 출력은 요청이 잘못된 게 아니라
모델이 흔들린 결과이므로, 파싱을 재시도 루프 *안*에서 수행합니다. 루프 밖에서 파싱하면
"Decode 오류는 재시도한다"는 분류가 무의미해집니다.

## 표준 어휘를 타입으로

같은 버그가 두 번 났습니다. 두 번 다 원인은 "역할·톤·스타일을 문자열로 비교한다"였습니다.

| | 사건 | 결과 |
|---|---|---|
| 2026-05 | 여성 아이템 시드가 `role`에 `base`/`accent`, `style`에 무드 분류를 넣음 | DB 281건 중 141건이 규칙에 투명 |
| 2026-08 | 커밋 `67b710b`이 밥/반찬 → 베이스/포인트 이름 변경 시 fixture 55건 누락 | hard filter 정확도 6.3%p 과소 측정 |

`Option<String>`인 한 오타든 다른 어휘든 그냥 "일치하지 않음"으로 흘러가고, 컴파일러도 테스트도
잡지 못합니다. 그래서 5개 어휘를 타입으로 승격했습니다
([`models/style_vocab.rs`](src/models/style_vocab.rs)):

```rust
pub enum Role { Base, Accent, SoftAccent, Connector, Structural }
//              베이스  포인트   약한포인트    연결템      구조템
```

이제 잘못된 값이 들어올 수 있는 **네 경계 모두에서 실패합니다**:

| 경계 | 이전 | 이후 |
|---|---|---|
| 코드 | `== Some("베이스")` 오타 → 항상 false | 존재하지 않는 variant → **컴파일 실패** |
| DB 읽기 | 표준 밖 값 → 조용히 무시 | sqlx `Decode` → **행 디코딩 실패** |
| DB 쓰기 | 무엇이든 저장 가능 | `CHECK` 제약 → **저장 거부** |
| API·LLM | 표준 밖 값 → 그대로 저장 | serde 역직렬화 실패 → **400 / 재시도** |

마지막 항목은 provider 레이어와 맞물립니다 — LLM이 `밝은`을 반환하면 파싱이 실패하고,
[재시도 루프](#model-orchestration-task-기반-provider-추상화) 안에서 다시 요청합니다.

기존 데이터는 [마이그레이션](migrations/20260827000001_normalize_style_vocabulary.sql)으로
정규화했습니다. 정직하게 대응되지 않는 값은 채우지 않고 비웠습니다 — `role='outer'` 24건은
카테고리가 잘못 들어간 것이고, `boho`/`romantic` 같은 무드 값 62건은 스타일 충돌 축에 대응이
없습니다. 임의로 채우면 바로잡으려던 신호를 다시 오염시킵니다.

전환 과정에서 죽은 비교도 하나 드러났습니다: `shoe_style == "아웃도어"` — `아웃도어`는
`texture_worlds`의 값이지 `Style`에 없어서, 이 조건은 참이 될 수 없었습니다.

> 330여 곳의 비교를 재작성했지만 **eval 스코어카드는 바이트 단위로 동일**합니다.
> 타입만 바뀌고 판정은 그대로라는 증거입니다.

## 핵심 원칙: LLM과 판단의 분리

```
규칙 엔진 (style_engine.rs)      LLM (services/llm 경유)
━━━━━━━━━━━━━━━━━━━━━━━━━━     ━━━━━━━━━━━━━━━━━━━━━━━
점수 / verdict / 강점 / 문제점     의류 이미지 서술
구조화된 suggestions 생성          스타일 태그 초기 추정
한 줄 총평(summary) 생성           규칙 결과를 자연어로 설명
  → 결정론적, 테스트 가능             → 판단하지 않음, 설명만
```

## 2-Pass RAG 의류 인식

```
이미지 업로드
    ↓
Pass 1: Vision → 변별력 있는 시각 특징 서술 (브랜드 추측 금지)
    ↓
Embedding → 1536차원 → 레퍼런스 코사인 유사도 검색 (인메모리)
    ↓
        유사도 < 0.5 → 레퍼런스 없이 일반 분석으로 폴백
    ↓
Pass 2: Vision + 레퍼런스 → 정밀 분석 (구조적 일치 시에만 모델명 사용)
    ↓
DB 저장 (의류 정보 + 스타일 메타데이터 11종 + 시즌 + 텍스처 월드)
```

1단계에서 브랜드 추측을 금지하는 이유는 검색 쿼리를 오염시키지 않기 위해서입니다. 브랜드명은
2단계에서 레퍼런스와 구조적 특징이 충분히 일치할 때만 붙습니다.

## 코디 평가 Flow

```
코디 입력 (상의/하의/아우터/신발/가방 + 상황)
    ↓
Style Engine: 15개 규칙 평가 → 점수(상한 95) + verdict + 강점 + 문제점
    ↓
Summary Generator: 규칙 기반 한 줄 총평 (결정론적)
    ↓
LLM Explanation: 자연어 설명 생성 (강점을 명시적으로 전달)
    ↓
응답: score, verdict, summary, strengths, problems, suggestions, explanation
```

---

# 품질 관리 (Eval + CI)

규칙 엔진은 96건의 라벨링된 케이스 카탈로그(`tests/fixtures/`)로 평가되며, **기준선 대비
하락하면 테스트가 실패합니다.** "좋아진 것 같다"가 아니라 수치로 확인합니다.

```bash
cargo test --test eval_scorecard -- --nocapture       # 실행 + 회귀 게이트
UPDATE_EVAL_BASELINE=1 cargo test --test eval_scorecard   # 의도한 변경 후 재기준선
```

현재 스코어카드 ([`tests/eval/scorecard.md`](tests/eval/scorecard.md)):

| 지표 | 값 |
|---|---|
| Hard filter 정확도 | **93.8%** (FP 3건 / FN 3건) |
| — 거절 정밀도 / 재현율 | 75.0% / 75.0% |
| Today-fit 정확도 (3-class) | **61.5%** |
| 선호도 순위 정확도 (Accept > Reject 쌍) | **74.9%** |
| Hard + fit 동시 일치 | **58.3%** |

선호도를 임계값 기반 정확도가 아니라 **쌍별 순위 정확도**로 잡았습니다. 임계값을 정하면 점수
스케일을 바꿀 때마다 지표가 무의미해지지만, 순위 지표는 스케일에 불변이라 엔진 개편 전후를
같은 자로 비교할 수 있습니다.

## 하네스가 첫 실행에서 잡아낸 것

첫 스코어카드는 hard filter 정확도 87.5%, 거절 정밀도 50%였습니다. 원인을 좇아가 보니 룰이
아니라 **데이터였습니다.** 커밋 `67b710b`이 role 용어를 밥/반찬 → 베이스/포인트로 바꾸면서
엔진만 고치고 테스트 fixture 55건을 그대로 두었습니다. 엔진이 모르는 문자열이라 그 아이템들은
베이스로도 포인트로도 세어지지 않았고, `LackOfStructure`가 멀쩡한 코디를 계속 탈락시키고
있었습니다.

fixture 용어를 맞춘 것만으로 (룰은 한 줄도 건드리지 않고):

| 지표 | 이전 | 이후 |
|---|---|---|
| Hard filter 정확도 | 87.5% | **93.8%** (+6.3%p) |
| 거절 정밀도 | 50.0% | **75.0%** (+25.0%p) |
| false positive | 8건 | **3건** |
| — 그중 `LackOfStructure` | 6건 | **1건** |
| Hard + fit 동시 일치 | 54.2% | **58.3%** |

이름 바꾸기 커밋이 조용히 남긴 회귀를 컴파일러도 기존 테스트도 잡지 못했습니다. 문자열 비교로
역할을 판정하는 한 이 계열의 버그는 또 납니다 — [알려진 문제](#알려진-문제)에 후속 작업으로
적어두었습니다.

## CI 게이트

[`.github/workflows/ci.yml`](.github/workflows/ci.yml)에서 매 푸시·PR마다 강제됩니다:

| 단계 | 기준 |
|---|---|
| `cargo fmt --check` | 포맷 미준수 시 실패 |
| `cargo clippy -- -D warnings` | 경고 0. 예외는 코드에 사유를 적은 `#[allow]`로만 |
| `cargo test --all-targets` | 114개 |
| eval 스코어카드 | 기준선 대비 회귀 시 실패 |
| 산출물 동기화 | 커밋된 스코어카드가 현재 엔진과 다르면 실패 |

테스트는 DB도 API 키도 요구하지 않습니다. 외부 의존이 필요한 검증은 `#[ignore]`로 분리합니다.

---

# 도메인 레퍼런스

## 스타일 메타데이터 (11종)

의류 등록 시 자동 추출됩니다. 초기 추정값이며 사용자가 수정할 수 있습니다.

| 속성 | 값 | 설명 |
|------|-----|------|
| `tone` | 밝음/중간/어두움 | 전체 밝기 |
| `saturation` | 낮음/중간/높음 | 색상 채도 |
| `style` | 베이직/워크/밀리터리/포멀/스포츠 | 대표 스타일 |
| `weight` | 가벼움/중간/무거움 | 시각적 무게감 |
| `role` | 베이스/포인트/약한포인트/연결템/구조템 | 코디에서의 역할 |
| `color_temperature` | warm/cool/neutral | 색온도 |
| `versatility` | universal/flexible/situational/statement | 활용도 |
| `statement_level` | 1~5 | 존재감 |
| `formality_level` | 1~5 | 격식 수준 |
| `texture_worlds` | workwear/military/tailoring/sweat/outdoor/minimal | 텍스처 월드 (복수) |
| `seasons` | 봄/여름/가을/겨울 | 계절 (복수) |

`role`이 이 엔진의 중심 개념입니다. **베이스**는 코디의 바탕이 되는 무난한 아이템,
**포인트**는 존재감으로 시선을 끄는 아이템, **구조템**은 실루엣과 무게중심을 잡아주는
아이템입니다. 대부분의 규칙이 이 역할 구성의 균형을 봅니다.

## 평가 규칙 (15개)

| 규칙 | Issue Code | 감점 | 비고 |
|------|-----------|------|------|
| 포인트 과다 | `TooManyAccents` | -20/-25 | 이너/하의에 포인트 시 더 큰 감점 |
| 중심축 부재 | `LackOfStructure` | -15 | 베이스·구조템 모두 없음 |
| 코디 단조로움 | `LackOfStructure` | -7 | 베이스+연결템만 (포인트/구조 없음) |
| 밝기 불균형 | `LackOfContrast` | -15 | 전부 어두움 / 전부 밝음 |
| 대비 부족 | `LackOfContrast` | -6~-15 | 톤·채도가 모두 동일 |
| 자연톤 과다 | `TooMuchNaturalTone` | -12/-18 | warm 편중 + 구조템 없으면 가중 |
| 스타일 충돌 | `StyleConflict` | -20 | 포멀+스포츠 등 |
| 텍스처 월드 충돌 | `TextureWorldConflict` | -5/-15 | tailoring+sweat 등 |
| 슬롯 역할 미스매치 | `SlotRoleMismatch` | -8 | 아이템 역할이 슬롯에 안 맞음 |
| 강한 이너 | `StrongInner` | -10 | 아우터 있을 때 포인트 이너 |
| 가방 적합도 | `BagConflict` | -8~+5 | 역할/스타일/톤/존재감 보정 |
| 신발 적합도 | `SlotRoleMismatch` | -15~+10 | 역할/대비/스타일/시즌 보정 |
| 세계관 과잉 | `WorldOvermatching` | -4~-15 | 밀리터리+올리브/카키 → 군복감 |
| 격식-상황 미스매치 | `FormalitySituationMismatch` | -12~-20 | 출근인데 캐주얼 등 |
| 시즌 보정 | `SeasonalMismatch` | -5/-15 | 계절 불일치 비율 |

**점수 체계**: 95점에서 시작해 감점 (100점은 없음) · **Verdict**: 88+ 훌륭해요 / 73+ 좋아요 /
55+ 괜찮아요 / ~54 아쉬워요

**슬롯별 기대 역할**

| 슬롯 | 기대 역할 |
|------|----------|
| 이너 (아우터 있을 때) | 베이스, 연결템 |
| 이너 (아우터 없을 때) | 베이스, 포인트, 약한포인트, 연결템 |
| 하의 | 베이스, 구조템, 연결템 |
| 아우터 | 포인트, 약한포인트, 구조템, 연결템 |
| 신발 / 가방 | 연결템, 구조템, 베이스 |

---

# API

## 의류

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/clothes/upload` | 이미지 → 2-Pass RAG 분석 → 자동 등록 + 스타일 태깅 |
| POST | `/api/clothes` | 수동 등록 |
| GET | `/api/clothes` | 전체 목록 |
| GET | `/api/clothes/{id}` | 단일 조회 |
| PUT | `/api/clothes/{id}` | 수정 |
| DELETE | `/api/clothes/{id}` | 삭제 |

## 코디 평가 · 추천

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/outfit/evaluate` | 코디 평가 (위 [예시](#실제-동작-예시) 참고) |
| POST | `/api/recommendation` | 단일 코디 추천 |
| POST | `/api/recommendation/multi` | 다중 후보 생성 → 엔진 채점 → 최적 선택 |
| POST | `/api/chat` | 도구 호출 기반 스타일링 대화 |
| POST | `/api/chat/image` | 착장 룩북 이미지 생성 (자동 검수 재시도 포함) |
| POST | `/api/feedback` | 코디 선호/비선호 피드백 저장 |

## 레퍼런스 (RAG 지식 베이스)

| Method | Path | 설명 |
|--------|------|------|
| GET | `/api/references` | 전체 목록 |
| POST | `/api/references` | 추가 (자동 임베딩) |
| PUT | `/api/references/{id}` | 수정 (재임베딩) |
| DELETE | `/api/references/{id}` | 삭제 |

## 기타

| Method | Path | 설명 |
|--------|------|------|
| GET | `/api/weather` | 현재 날씨 |
| GET · PUT | `/api/region` | 지역 조회 / 설정 |
| GET | `/api/style-moods` | 무드 목록 |
| POST | `/api/user/register` | 사용자 등록 (API 토큰 발급) |
| GET | `/api/health` | 헬스 체크 |

## UI (4화면 SPA)

| 화면 | 라우트 | 역할 |
|------|--------|------|
| 홈 | `#home` | 날씨 + CTA + 추천 카드 + 옷장 요약 |
| 코디 평가 | `#evaluate` | 슬롯 선택 → 점수/문제/제안/설명 |
| 옷장 | `#wardrobe` | 카테고리·역할 필터 + 등록(수동/이미지) |
| 아이템 상세 | `#detail/{id}` | 역할 해석 + 스타일 태그 + "이 옷으로 평가" |

핵심 루프: 홈 → 평가 → 개선 제안 → 옷장 → 아이템 교체 → 다시 평가

---

# 프로젝트 구조

```
src/
├── main.rs                       # 서버 초기화, AppState 구성
├── services/
│   ├── llm/                      # ★ provider 추상화 레이어
│   │   ├── mod.rs                #   LlmClient — 라우팅·재시도·타임아웃·계측
│   │   ├── config.rs             #   task → provider/model 라우팅 테이블
│   │   ├── provider.rs           #   Chat/Embedding/Image provider trait
│   │   ├── openai.rs             #   OpenAI 구현 (chat·embedding·image)
│   │   ├── anthropic.rs          #   Anthropic 구현 (chat·vision)
│   │   ├── types.rs              #   provider 중립 요청/응답 타입
│   │   ├── usage.rs              #   토큰·지연·추정비용 계측
│   │   └── error.rs              #   재시도 가능성 분류
│   ├── prompts.rs                # 스타일 도메인 LLM task 정의 (프롬프트 전용)
│   ├── style_engine.rs           # ★ 15개 규칙 엔진 + 강점 + suggestions + summary
│   ├── style_engine_v2.rs        # 3계층 분리 실험 (hard filter / subscore)
│   ├── serving_ranker.rs         # today_fit + serving 보정
│   ├── recommendation_experiment.rs  # shadow mode 로그 수집기
│   ├── embedding.rs              # 임베딩 캐시 + 코사인 유사도 검색
│   └── weather.rs                # 기상청 API (Open-Meteo 폴백)
├── routes/                       # home(SPA) / outfit / clothes / chat / reference / ...
├── models/
│   ├── style_vocab.rs            # ★ 표준 어휘 타입 (Role/Tone/Style/Weight/Saturation)
│   └── clothing · outfit · recommendation · reference · weather
└── db/                           # sqlx 리포지토리

tests/
├── common/mod.rs                 # fixture 로딩 (두 테스트 바이너리가 공유)
├── fixtures/                     # 아이템 카탈로그 138건 + 라벨링 케이스 96건
├── eval_scorecard.rs             # ★ eval 하네스 + 회귀 게이트
├── eval/                         # 스코어카드 산출물 + 커밋된 기준선
├── shadow_cases.rs               # 진단용 리포트 (판정하지 않음)
└── style_engine_test.rs          # 규칙별 단위 테스트

migrations/                       # MySQL 마이그레이션
```

---

# 시작하기

## 사전 요구사항

- Rust (Edition 2024)
- MySQL 8.0+
- OpenAI API Key
- Anthropic API Key (선택 — 해당 provider로 라우팅할 때만)
- 기상청 API Key (선택 — 미설정 시 Open-Meteo 폴백)

## 실행

```bash
cp .env.example .env      # DATABASE_URL, OPENAI_API_KEY 등 설정
mysql -u root -e "CREATE DATABASE rust_web_app"
cargo run                 # 마이그레이션·시드·임베딩 생성 자동
```

`http://localhost:3003`에서 시작됩니다. 첫 실행 시 DB 마이그레이션, 밀리터리/빈티지 레퍼런스
13종 시드, 레퍼런스 임베딩 생성 + 인메모리 캐시 로딩이 자동으로 수행됩니다.

기동 시 어떤 task가 어떤 모델로 라우팅되는지 로그로 확인할 수 있습니다:

```
INFO LLM 라우팅 task="vision_pass1" provider="openai" model=gpt-4o-mini configured=true
INFO llm_call task="embedding" provider="openai" model="text-embedding-3-small"
     input_tokens=369 output_tokens=0 latency_ms=3454 attempts=1 cost_usd=0.00000738
```

## 테스트

```bash
cargo test                                          # 전체 114개
cargo test --test eval_scorecard -- --nocapture     # eval 스코어카드
```

---

# 알려진 문제

솔직하게 남겨둡니다. 수치와 로그로 확인된 것만 적었습니다.

### ~~1. 메타데이터 어휘 분열~~ → 해결됨

라이브 DB 281건 중 141건이 규칙 엔진이 인식하지 못하는 어휘(`base`/`accent`/`outer`,
`밝은`/`어두운`, `boho`/`office`/`street` …)를 쓰고 있었습니다. 옷장 절반이 역할 기반 규칙에
사실상 투명했습니다. 위 fixture 사건과 같은 근본 원인이고 규모만 컸습니다.
자세한 경위는 [`models/style_vocab.rs`](src/models/style_vocab.rs) 상단 주석에 남겼습니다.

**해결**: 어휘를 타입으로 승격했습니다 ([어휘 타입](#표준-어휘를-타입으로) 참고).

### 2. `thickness`에 같은 어휘 분열이 남아 있음

어휘 타입 승격 대상에서 빠졌습니다. 281건이 두 어휘로 갈려 있습니다 —
`medium` 137 / `중간` 97 / `얇은` 39 / `두꺼운` 5 / `thick` 2 / `thin` **1**.

이 필드는 실제로 규칙에서 비교됩니다:

```rust
serving_ranker.rs:77          t.clothing.thickness == "thin"   // 온도 게이트
recommendation_service.rs:99  t.thickness == "thin"            // 적합도 게이트
```

`thin`이 1건뿐이므로 `얇은` 39건에 대해 "얇은 상의 + 아우터 없음 → 온도 부적합" 판정이
죽어 있습니다. 원인도 코드 안에 있습니다 — 프롬프트 두 개가 서로 다른 값을 요구합니다
(`"두께"` 자유 텍스트 vs `"thin/medium/thick 중 하나"`).

단 **eval 수치에는 영향이 없습니다.** fixture가 `thickness`를 `"medium"`으로 하드코딩해
이 축을 밟지 않기 때문입니다. 즉 eval의 커버리지 구멍이기도 합니다.

### 3. Today-fit이 지나치게 관대 (61.5%)

오분류 37건 중 32건이 `→Pass` 방향입니다(`Borderline→Pass` 23, `Fail→Pass` 9). 통과 임계선이
낮게 잡혀 있습니다. role 수정과 무관하게 그대로였습니다.

### 4. 선호도 분리도 부족 (4.8점)

Accept 평균 96.7 vs Reject 평균 91.9. 상한 95에 눌려 대부분의 코디가 90점대에 몰립니다. style
score가 좋은 코디와 나쁜 코디를 거의 구분하지 못하므로 점수 스케일 재설계가 필요합니다.

### 5. 운영 관점의 공백

- Anthropic 경로는 단위 테스트로만 검증됨 (API 키 없이 실호출 미검증)
- LLM 호출에 동시성 제한이 없음
- 임베딩 인메모리 캐시는 수평 확장 불가 — 단일 인스턴스 전제
- 비용 계측은 하지만 예산 가드레일은 없음

---

# 설계 원칙

- **판단과 서술의 분리** — 점수·판정은 결정론적 규칙, LLM은 설명 생성만. 테스트 가능성이 목적
- **모델은 설정, 코드가 아님** — 호출부는 task만 알고 모델명을 모른다
- **능력 차이는 경계에서 흡수** — provider별 형식 차이는 구현체가 삼키고 호출부는 모른다
- **품질은 수치로** — 엔진 변경은 스코어카드로 확인하고, 회귀는 CI가 막는다
- **환각 방지** — 브랜드는 시각적 확인 시에만, 레퍼런스는 강한 일치 시에만 매칭
- **임베딩은 외부 API** — 로컬 모델을 상주시키지 않아 저사양 배포가 단순 (초기 fastembed ONNX
  로컬 추론에서 전환, 저사양 EC2 배포 비용이 근거)
- **엔진 개편은 shadow mode로** — baseline과 병행 실행해 로그만 남기고, in-place 교체는 금지
