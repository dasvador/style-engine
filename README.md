# Style Engine

추천이 아닌 판단을 제공하는 AI 코디 평가 엔진

> "이렇게 입으세요"가 아니라 "왜 좋고, 왜 별로인지" 설명하는 서비스

## 핵심 기능

- **2-Pass RAG 의류 인식** — 이미지 업로드 → Vision AI 서술 → 임베딩 검색 → 정밀 분석
- **스타일 태깅** — 의류 등록 시 tone, role, color_temperature 등 11개 메타데이터 자동 추출
- **코디 평가 엔진** — 13개 규칙 기반 점수화 + 강점/문제점/구조화된 개선안 제공
- **설명 가능한 AI** — 규칙 엔진이 판단, LLM은 결과를 자연어로 설명 (밥/반찬 비유 활용)
- **4화면 SPA** — 홈 / 코디 평가 / 옷장 / 아이템 상세

## Tech Stack

| 구분 | 기술 |
|------|------|
| Language | Rust (Edition 2024) |
| Framework | Axum + Tokio |
| Database | MySQL (sqlx) |
| Vision AI | OpenAI gpt-4o-mini |
| Embedding | fastembed (multilingual-e5-small, 384차원, ONNX 로컬) |
| Weather | 기상청 초단기실황 API (Open-Meteo 폴백) |

## 아키텍처

### 핵심 원칙: LLM과 규칙 엔진의 분리

```
규칙 엔진 (style_engine.rs)     LLM (openai.rs)
━━━━━━━━━━━━━━━━━━━━━━━━━     ━━━━━━━━━━━━━━━━━━━━━
점수/verdict/강점/문제점 결정     의류 이미지 서술
구조화된 suggestions 생성        스타일 태그 초기 추정
한 줄 총평 (summary) 생성        규칙 결과를 자연어 설명
  → 결정론적, 테스트 가능           → 판단하지 않음, 설명만
```

### 2-Pass RAG Flow

```
이미지 업로드
    ↓
Pass 1: Vision API → 변별력 있는 시각 특징 서술 (브랜드 추측 금지)
    ↓
fastembed → 384차원 벡터 → 레퍼런스 코사인 유사도 검색 (인메모리)
    ↓
Pass 2: Vision API + 레퍼런스 → 정밀 분석 (구조적 일치 시에만 레퍼런스 매칭)
    ↓
DB 저장 (의류 정보 + 11개 스타일 메타데이터 + 시즌 + 텍스처 월드)
```

### 코디 평가 Flow

```
코디 입력 (상의/하의/아우터/신발/가방 + 상황)
    ↓
Style Engine: 13개 규칙 평가 → 점수(상한 95) + verdict + 강점 + 문제점
    ↓
Summary Generator: 규칙 기반 한 줄 총평 (결정론적)
    ↓
LLM Explanation: 자연어 설명 생성 (강점 명시적 전달)
    ↓
응답: score, verdict, summary, strengths, problems, suggestions, explanation
```

## UI 구조 (4화면 SPA)

| 화면 | 라우트 | 역할 |
|------|--------|------|
| **홈** | `#home` | 날씨 + CTA(평가/추천) + 추천 카드 + 옷장 요약(밥/반찬 비율) |
| **코디 평가** | `#evaluate` | 슬롯 선택 → 점수/verdict/강점/문제/구조화된 제안/설명 |
| **옷장** | `#wardrobe` | 카테고리·역할 필터 + 카드 리스트 + 등록(수동/이미지) |
| **아이템 상세** | `#detail/{id}` | 역할 해석 + 스타일 태그 + "이 옷으로 평가" 버튼 |

핵심 UX 루프: 홈 → 코디 평가 → 개선 제안 → 옷장 → 아이템 선택 → 다시 평가

## 스타일 메타데이터

의류 등록 시 자동으로 추출되는 11개 속성 (초기 추정값, 사용자 수정 가능):

| 속성 | 값 | 설명 |
|------|-----|------|
| tone | 밝음/중간/어두움 | 전체 밝기 |
| saturation | 낮음/중간/높음 | 색상 채도 |
| style | 베이직/워크/밀리터리/포멀/스포츠 | 대표 스타일 |
| weight | 가벼움/중간/무거움 | 시각적 무게감 |
| role | 밥/반찬/약한반찬/연결템/구조템 | 코디에서의 역할 |
| color_temperature | warm/cool/neutral | 색온도 |
| versatility | universal/flexible/situational/statement | 활용도 |
| statement_level | 1~5 | 존재감 |
| formality_level | 1~5 | 격식 수준 |
| texture_worlds | workwear/military/tailoring/sweat/outdoor/minimal | 텍스처 월드 (복수) |
| seasons | 봄/여름/가을/겨울 | 계절 (복수) |

## 평가 규칙 (13개)

| 규칙 | Issue Code | 감점 | 비고 |
|------|-----------|------|------|
| 반찬 과다 | TooManyAccents | -20/-25 | 이너/하의에 반찬 시 더 큰 감점 |
| 중심축 부재 | LackOfStructure | -15 | 밥 없음 + 가벼움 + accent/warm 편중 |
| 밝기 불균형 | LackOfContrast | -15/-10 | 전부 어두움 / 전부 밝음 |
| 대비 부족 | LackOfContrast | -6~-15 | 중간톤만 2~3아이템에서도 감지 |
| 자연톤 과다 | TooMuchNaturalTone | -12/-18 | 구조 없으면 가중 |
| 스타일 충돌 | StyleConflict | -20 | 포멀+스포츠, 워크 중복 등 |
| 텍스처 월드 충돌 | TextureWorldConflict | -15 | tailoring+sweat 등 |
| 슬롯 역할 미스매치 | SlotRoleMismatch | -8 | 이너→밥/연결템, 하의→밥/구조템 |
| 강한 이너 | StrongInner | -10 | 아우터 있을 때 반찬 이너 |
| 가방 부조화 | BagConflict | -6/-12 | 다른 반찬 있으면 severe |
| 신발 밸런스 | SlotRoleMismatch | -8 | 반찬 신발, 스타일 충돌 |
| 세계관 과잉 | WorldOvermatching | -8/-15 | 밀리터리+올리브/카키 → 군복감 |
| 격식-상황 미스매치 | FormalitySituationMismatch | -12 | 출근인데 캐주얼 등 |
| 시즌 보정 | SeasonalMismatch | -5/-15 | 계절 불일치 비율 |

**점수 체계**: 상한 95 (100은 없음)

**Verdict**: 88+ 훌륭해요 / 73+ 좋아요 / 55+ 괜찮아요 / ~54 아쉬워요

**슬롯 기대 역할**:

| 슬롯 | 기대 역할 |
|------|----------|
| 이너 (아우터 있을 때) | 밥, 연결템 |
| 이너 (아우터 없을 때) | 밥, 반찬, 약한반찬, 연결템 |
| 하의 | 밥, 구조템, 연결템 |
| 아우터 | 반찬, 약한반찬, 구조템, 연결템 |
| 신발/가방 | 연결템, 구조템, 밥 |

## API Endpoints

### 의류

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/clothes/upload` | 이미지 → RAG 분석 → 자동 등록 + 스타일 태깅 |
| POST | `/api/clothes` | 수동 등록 |
| GET | `/api/clothes` | 전체 목록 |
| GET | `/api/clothes/{id}` | 단일 조회 |
| PUT | `/api/clothes/{id}` | 수정 |
| DELETE | `/api/clothes/{id}` | 삭제 |

### 코디 평가

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/outfit/evaluate` | 코디 평가 (점수 + 강점 + 문제 + 구조화된 제안 + 설명) |

```json
// Request
{
  "top": "clothing-id",
  "bottom": "clothing-id",
  "outer": "clothing-id",
  "situation": "출근"
}

// Response
{
  "score": 83,
  "verdict": "Good",
  "verdict_label": "좋아요",
  "summary": "올리브 M-65 필드 자켓 인디고 셀비지 데님와의 조합에서 — 상황과 격식 수준이 맞지 않습니다.",
  "strengths": [
    { "rule": "밝기 밸런스", "detail": "밝음과 어두움의 대비가 잘 잡혀있어 시각적으로 깔끔합니다" }
  ],
  "problems": [
    { "code": "FormalitySituationMismatch", "rule": "격식 수준", "deduction": 12, "detail": "출근에 비해 너무 캐주얼합니다" }
  ],
  "suggestions": [{
    "type": "upgrade_formality",
    "reason_code": "FormalitySituationMismatch",
    "reason": "상황에 비해 너무 캐주얼합니다",
    "recommended_roles": ["구조템"],
    "recommended_colors": ["네이비", "차콜"],
    "recommended_examples": ["옥스포드 셔츠", "테일러드 자켓"]
  }],
  "explanation": "LLM이 생성한 자연어 설명..."
}
```

### 레퍼런스 (RAG 지식 베이스)

| Method | Path | 설명 |
|--------|------|------|
| GET | `/api/references` | 전체 목록 |
| POST | `/api/references` | 추가 (자동 임베딩) |
| PUT | `/api/references/{id}` | 수정 (재임베딩) |
| DELETE | `/api/references/{id}` | 삭제 |

### 추천 / 날씨 / 기타

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/recommendation` | 스타일 메타데이터 기반 코디 추천 |
| GET | `/api/weather` | 현재 날씨 |
| PUT | `/api/region` | 지역 설정 |
| GET | `/api/health` | 헬스 체크 |

## 프로젝트 구조

```
src/
├── main.rs                     # 서버 초기화, 상태 구성
├── errors.rs                   # 에러 타입
├── models/
│   ├── clothing.rs             # 의류 모델 + 스타일 태그 11종
│   ├── outfit.rs               # 코디 평가 모델 (Verdict, IssueCode, StructuredSuggestion 등)
│   ├── recommendation.rs       # 추천 모델
│   ├── reference.rs            # RAG 레퍼런스 모델
│   ├── weather.rs              # 날씨 모델
│   └── region.rs               # 지역 설정
├── services/
│   ├── style_engine.rs         # 13개 규칙 엔진 + 강점 감지 + 구조화된 suggestions + summary
│   ├── openai.rs               # Vision API (2-Pass) + 코디 추천 + 설명 생성
│   ├── embedding.rs            # fastembed 래퍼, 캐시, 검색
│   └── weather.rs              # 기상청 초단기실황 API (Open-Meteo 폴백)
├── routes/
│   ├── home.rs                 # 4화면 SPA (홈/평가/옷장/상세)
│   ├── outfit.rs               # POST /api/outfit/evaluate
│   ├── clothes.rs              # 의류 CRUD + 이미지 업로드
│   ├── reference.rs            # 레퍼런스 CRUD
│   ├── recommendation.rs       # 코디 추천 (메타데이터 기반)
│   └── ...
└── db/
    ├── clothing_repo.rs        # 의류 + 시즌 + 텍스처월드 DB
    ├── reference_repo.rs       # 레퍼런스 DB
    └── region_repo.rs          # 지역 DB

migrations/                     # MySQL 마이그레이션 (7개)
```

## 시작하기

### 사전 요구사항

- Rust (Edition 2024)
- MySQL 8.0+
- OpenAI API Key
- 기상청 API Key (공공데이터포털, 선택 — 미설정 시 Open-Meteo 폴백)

### 설정

```bash
# 환경변수
cp .env.example .env
# .env 파일에서 DATABASE_URL, OPENAI_API_KEY, KMA_API_KEY 설정

# DB 생성
mysql -u root -e "CREATE DATABASE rust_web_app"

# 실행 (마이그레이션 자동 + 시드 데이터 자동 + 임베딩 모델 자동 다운로드)
cargo run
```

서버가 `http://localhost:3000`에서 시작됩니다.

### 첫 실행 시

1. 임베딩 모델 (~80MB) 자동 다운로드 → `.fastembed_cache/`에 캐시
2. DB 마이그레이션 자동 실행
3. 밀리터리/빈티지 레퍼런스 13종 자동 시드
4. 레퍼런스 임베딩 자동 생성 + 인메모리 캐시 로딩

## 설계 원칙

- **규칙-LLM 분리**: 점수/판정은 결정론적 규칙, LLM은 설명 생성만
- **로컬 임베딩**: API 비용 없이 한국어 임베딩 (fastembed ONNX)
- **인메모리 캐시**: 레퍼런스 검색 시 DB 조회 없이 코사인 유사도 계산
- **환각 방지**: 브랜드는 시각적 확인 시에만 포함, 레퍼런스는 강한 일치 시에만 매칭
- **설명 가능**: 모든 감점/강점에 이유와 구조화된 개선안 제공
- **점진적 확장**: 기존 코드 유지하면서 레이어 추가 방식
