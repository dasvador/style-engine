# Wardrobe Edit

> Powered by **Style Engine**

[![CI](https://github.com/dasvador/style-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/dasvador/style-engine/actions/workflows/ci.yml)

Wardrobe Edit은 사용자의 옷장과 그날의 상황을 바탕으로 코디를 제안하고, 각 선택의 이유를 설명하는 AI 스타일링 서비스입니다.

옷 사진을 등록하면 Vision 모델이 색감, 소재, 실루엣, 격식 등 스타일 정보를 정리합니다. 추천 단계에서는 날씨와 상황, 사용자의 스타일 선호, 최근 추천 이력을 함께 고려합니다. 결과는 점수만 제시하지 않고 조화로운 부분과 조정이 필요한 부분, 대체할 수 있는 아이템의 조건까지 보여줍니다.

> 이 프로젝트는 현재 개발 중인 개인 프로젝트입니다. 추천 결과는 스타일 선택을 돕기 위한 참고 정보이며, 사용자의 취향을 대신해 정답을 제시하는 것을 목표로 하지 않습니다.

## 주요 기능

- **디지털 옷장**: 옷 사진 분석, 스타일 속성 자동 정리, 직접 수정
- **상황별 코디 추천**: 출근, 데이트, 일상 등 상황과 날씨를 반영한 추천
- **스타일 장르**: 성별별 8개 대표 장르 중 선택한 장르에 맞는 아이템으로 추천 범위를 좁힘
- **코디 평가**: 색감, 균형, 활용도, 액세서리 조화를 기준으로 점수와 근거 제공
- **추천 이력 반영**: 최근에 반복된 아이템은 낮추고 새로운 조합에는 가산점 부여
- **대화형 스타일링**: 옷장 검색과 코디 평가 도구를 사용하는 AI 상담
- **룩북 이미지**: 선택된 코디를 착장 이미지로 생성하고 자동 검수

## 추천 방식

Wardrobe Edit은 LLM의 응답을 그대로 추천 결과로 사용하지 않습니다. 후보 생성과 평가, 최종 노출 순위를 서로 다른 단계로 나눴습니다.

```text
사용자 옷장
+ 날씨와 상황
+ 스타일 선호
+ 최근 추천 이력
        ↓
후보 아이템 선별
        ↓
LLM 코디 후보 생성
        ↓
규칙 기반 적합성 검사와 점수 계산
        ↓
최근 노출·다양성·당일 적합도 반영
        ↓
추천 결과와 선택 이유 제공
```

### 1. 후보 아이템 선별

옷장 전체를 모델에 전달하지 않고 상황, 기온, 아이템 역할, 최근 사용 여부를 기준으로 후보를 줄입니다. 입력 크기를 제한하면서도 필요한 카테고리가 고르게 포함되도록 구성했습니다.

### 2. 코디 후보 생성

LLM은 선별된 아이템 안에서 복수의 코디 후보를 구성합니다. 호출부는 특정 모델명이 아니라 `VisionPass1`, `Recommendation`, `StyleNote`와 같은 작업을 지정합니다. 실제 provider와 모델은 설정에서 교체할 수 있습니다.

### 3. 규칙 기반 평가

각 후보는 다음 네 가지 축으로 평가합니다.

| 평가 축 | 확인하는 내용 |
| --- | --- |
| Balance | 아이템의 역할과 시각적 무게가 균형을 이루는지 |
| Coherence | 색온도, 스타일, 소재가 자연스럽게 연결되는지 |
| Utility | 계절, 기온, 상황과 격식에 적합한지 |
| Accessory | 신발과 가방이 전체 조합을 보완하는지 |

명확한 충돌은 hard filter에서 제외하고, 통과한 후보에는 0~100 범위의 스타일 점수를 계산합니다.

### 4. 최종 순위 조정

기본 점수에 최근 추천 페널티와 다양성 보너스, 당일 날씨 적합도를 반영합니다. 같은 아이템과 비슷한 조합이 반복되는 현상을 줄이기 위한 단계입니다.

## 코디 평가 예시

```json
{
  "score": 74,
  "verdict": "Good",
  "summary": "전체적인 색감은 안정적이지만 출근 상황에 비해 신발이 다소 캐주얼합니다.",
  "strengths": [
    "상의와 하의의 색온도가 자연스럽게 연결됩니다."
  ],
  "problems": [
    {
      "code": "FormalitySituationMismatch",
      "deduction": 12,
      "detail": "현재 신발은 나머지 아이템보다 격식 수준이 낮습니다."
    }
  ],
  "suggestions": [
    {
      "type": "upgrade_formality",
      "recommended_colors": ["네이비", "차콜"],
      "recommended_examples": ["로퍼", "미니멀한 가죽 스니커즈"]
    }
  ]
}
```

점수, 판정, 문제와 제안은 규칙 엔진이 생성합니다. LLM은 이 구조화된 결과를 읽기 쉬운 설명으로 정리합니다. 설명 생성에 실패하더라도 평가 결과는 그대로 사용할 수 있습니다.

## 이미지 분석

등록한 옷은 두 단계로 분석합니다.

```text
이미지 업로드
    ↓
Vision 1차 분석: 눈에 보이는 특징 정리
    ↓
Embedding 검색: 유사한 의류 레퍼런스 탐색
    ↓
Vision 2차 분석: 레퍼런스와 함께 세부 속성 확인
    ↓
사용자 옷장에 저장
```

첫 분석에서는 확인되지 않은 브랜드나 모델명을 추측하지 않습니다. 유사도가 충분하지 않으면 레퍼런스를 사용하지 않고 일반 분석 결과를 저장합니다. 자동으로 생성된 속성은 사용자가 직접 수정할 수 있습니다.

## 품질 검증

스타일 판단은 주관적일 수 있지만, 엔진 변경이 기존 결과를 예상치 못하게 훼손하는지는 반복해서 확인할 수 있어야 합니다.

- 96개의 라벨링된 평가 케이스 운영
- hard filter, 당일 적합도, 선호 조합 순위 지표 측정
- 기준선보다 결과가 나빠지면 CI 실패
- 규칙별 단위 테스트와 평가 스코어카드를 분리
- 새로운 랭킹 로직은 기존 결과와 함께 실행하는 shadow mode로 비교

현재 저장된 스코어카드:

| 지표 | 결과 |
| --- | ---: |
| Hard filter 정확도 | 94.8% |
| Today-fit 정확도 | 74.0% |
| 선호 조합 순위 정확도 | 74.9% |
| Hard filter와 Today-fit 동시 일치 | 70.8% |

이 수치는 실제 사용자 만족도가 아니라 저장된 평가 케이스에 대한 결과입니다. 평가 데이터의 범위와 라벨 품질에 따라 달라질 수 있습니다.

## LLM 연동

모든 모델 호출은 공통 client를 거칩니다.

```text
호출부: 작업 종류만 지정
        ↓
LlmClient
- 작업별 provider/model 선택
- timeout과 지수 백오프 재시도
- 응답 스키마 검증
- 토큰·지연시간·추정 비용 기록
        ↓
OpenAI / Anthropic provider
```

Chat, Embedding, Image 기능을 별도 interface로 구분했습니다. provider가 지원하지 않는 기능을 컴파일 시점과 설정 단계에서 명확히 구분하기 위한 구조입니다.

## 기술 구성

| 영역 | 기술 |
| --- | --- |
| Backend | Rust, Axum, Tokio |
| Frontend | React 18, TypeScript, Vite |
| Database | MySQL, sqlx |
| AI | OpenAI API, Anthropic API, Vision, Embedding, Image Generation |
| External data | 기상청 초단기실황 API, Open-Meteo fallback |
| Delivery | Docker, GitHub Actions |

운영 환경에서는 Axum이 API와 React SPA를 같은 origin에서 제공합니다. 프런트엔드와 백엔드는 코드로 분리하되 하나의 컨테이너로 배포할 수 있습니다.

```text
Browser
  ├─ /api/*      → Axum handlers → MySQL / external APIs
  ├─ /static/*   → uploaded and generated images
  └─ other paths → React SPA
```

## 주요 API

| Method | Path | Description |
| --- | --- | --- |
| POST | `/api/clothes/upload` | 이미지 분석 후 옷장에 등록 |
| GET | `/api/clothes` | 옷장 목록 조회 |
| POST | `/api/outfit/evaluate` | 선택한 코디 평가 |
| POST | `/api/recommendation/multi` | 복수 후보 생성·평가 후 추천 |
| POST | `/api/chat` | 도구 호출 기반 스타일링 대화 |
| POST | `/api/chat/image` | 착장 룩북 이미지 생성 |
| POST | `/api/feedback` | 선호·비선호 피드백 저장 |
| GET | `/api/style-moods` | 성별별 스타일 장르 목록 조회 |
| GET | `/api/weather` | 현재 날씨 조회 |
| GET | `/api/health` | 서비스 상태 확인 |

## 로컬 실행

### 요구사항

- Rust (`rust-toolchain.toml`에 버전 고정)
- Node.js 20 이상
- MySQL 8.0 이상
- OpenAI API key

### Backend

```bash
cp .env.example .env
mysql -u root -e "CREATE DATABASE rust_web_app"
cargo run
```

Backend는 기본적으로 `http://localhost:3003`에서 실행됩니다.

### Frontend

```bash
cd frontend
npm install
npm run dev
```

개발 서버는 `http://localhost:5173`에서 실행되며 `/api` 요청을 backend로 전달합니다.

### Production build

```bash
cd frontend && npm run build && cd ..
cargo build --release
./target/release/style-engine
```

### Docker

```bash
docker build -t style-engine .
docker run --rm -p 3003:3003 --env-file .env \
  -v "$PWD/static/images:/app/static/images" \
  style-engine
```

## 테스트

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --test eval_scorecard -- --nocapture

cd frontend
npm run build
npm run lint
```

GitHub Actions에서 format, lint, 전체 테스트와 평가 스코어카드 회귀 검사를 실행합니다.

## License

[MIT](LICENSE)
