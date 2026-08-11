# Military Vintage Clothing Style Coach System

밀리터리 빈티지 의류 스타일 코칭 시스템 아키텍처 문서

## Tech Stack

| 구분 | 기술 |
|------|------|
| Framework | Axum (Rust) + Tokio |
| Database | MySQL (sqlx, 런타임 쿼리) |
| Vision AI | OpenAI gpt-4o-mini |
| Embedding | OpenAI text-embedding-3-small (1536차원, API) |
| Weather | KMA 초단기실황 API |
| Style Engine | 결정론적 룰 엔진 (15+ 룰, 0-95점) |

---

## 시스템 개요

```
┌──────────────────────────────────────────────────────────┐
│                    웹 UI (home.rs)                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ 홈/추천   │ │ 옷장관리  │ │ 코디평가  │ │ 설정     │   │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘   │
└───────┼────────────┼────────────┼────────────┼──────────┘
        ▼            ▼            ▼            ▼
┌──────────────────────────────────────────────────────────┐
│                    Axum Routes                            │
│  /recommendation/multi  /clothes  /outfit/evaluate  ...  │
└───────┬────────────┬────────────┬────────────────────────┘
        ▼            ▼            ▼
┌────────────┐ ┌──────────┐ ┌────────────────┐
│ OpenAI API │ │ Style    │ │ Recommendation │
│ (LLM)     │ │ Engine   │ │ Service        │
└────────────┘ └──────────┘ └────────────────┘
        │            │            │
        ▼            ▼            ▼
┌──────────────────────────────────────────────────────────┐
│                    MySQL Database                         │
│  clothing · clothing_season · clothing_texture_world      │
│  clothing_reference · outfit_recommendation_history       │
│  region_setting                                           │
└──────────────────────────────────────────────────────────┘
```

---

## Style Engine (결정론적 룰 엔진)

코디 품질을 결정론적으로 평가. LLM에 의존하지 않음.

### 점수 모델

```
시작: 95점 (ceiling)
  - 각 룰 위반 시 감점
  + 신발 적합도 보너스 (-15 ~ +10)
  + 가방 적합도 보너스 (-8 ~ +5)
= 최종 점수 (0 ~ 95)
```

### 평가 룰 (15개)

| # | 룰 | 감점 | 설명 |
|---|-----|------|------|
| 1 | 베이스/포인트 밸런스 | -20/-25 | 포인트 2개 이상 = 과다 |
| 2a | 중심축 부재 | -15 | 베이스도 구조템도 없음 |
| 2b | 코디 단조로움 | -7 | 베이스+연결템만 (포인트/구조 없음) |
| 3 | 밝기 밸런스 | -15 | 전부 어두움(-15) 또는 전부 밝음(-15) |
| 4 | 대비 부족 | -6~-15 | 톤+채도 모두 동일 |
| 5 | 자연톤 과다 | -12/-18 | warm 편중 + 구조템 없음 |
| 6 | 스타일 충돌 | -20 | 포멀+스포츠 등 |
| 7 | 텍스처 월드 충돌 | -5/-15 | sweat+tailoring(-15), military+tailoring(조화로 인정) |
| 8 | 슬롯 역할 미스매치 | -8 | 아이템 역할이 슬롯에 안 맞음 |
| 9 | 이너 규칙 | -10 | 아우터 있을 때 이너가 포인트+고채도 |
| 10a | 가방 적합도 | -8~+5 | 역할/스타일/톤/존재감/밝기 보정 |
| 10b | 신발 적합도 | -15~+10 | 역할/대비 보강/스타일/시즌 |
| 11 | 세계관 과잉 | -4~-15 | 밀리터리/워크 아우터+같은 톤 하의 |
| 12 | 격식 수준 | -12~-20 | 상황별 격식 미스매치 (gap 기반 추가 감점) |
| 13 | 시즌 보정 | -5/-15 | 현재 시즌 미포함 아이템 비율 |

### Verdict 매핑

| 점수 | Verdict | 라벨 |
|------|---------|------|
| 88-95 | Great | 훌륭해요 |
| 73-87 | Good | 좋아요 |
| 55-72 | Okay | 괜찮아요 |
| 0-54 | Awkward | 아쉬워요 |

---

## 3-모드 추천 시스템

### 아키텍처

```
LLM 5후보 생성 (1회 호출)
  ↓
각 후보: style_engine 점수 + 이력 penalty/bonus
  ↓ (신발/가방 없으면 결정론적 매칭)
  ↓
순차 모드 선택:
  Step 1: select_todays_pick (적합도 게이트)
  Step 2: select_variation (Today와 상하의 다른 것)
  Step 3: select_dormant (휴면 아이템 포함)
  ↓
Mode 1 winner만 이력 저장
  ↓
MultiModeRecommendationResponse (3개 모드)
```

### 모드 정의

| 모드 | 라벨 | 선택 로직 | 품질 하한 |
|------|------|-----------|-----------|
| Today | 오늘의 추천 | 적합도 2/3 게이트 + 최고 adjusted score | 70 |
| Variation | 다른 조합 | Today와 상하의 쌍 다른 후보 중 최고 | 68 |
| Dormant | 안 입은 옷 활용 | 14일+ 미사용 아이템 포함 후보 | 65 |

### Today 적합도 게이트

3축 평가 — 최소 2/3 통과 필요:

| 축 | 체크 | 실패 시 감점 |
|---|---|---|
| 온도 적합 | ≤13도 + 얇은 상의 + 아우터 없음 | -20 (reject) |
| | 13~18도 + 얇은 상의 단독 | -8 |
| 대비 충분 | 상하의 동일 톤+색온도 | -12 |
| | 둘 다 밝음 (아우터 보완 없음) | -8 |
| 구조감 | 구조템/포멀/무거움 없음 | -7 |

### 추천 이력 시스템

| 패널티 | 감점 |
|--------|------|
| 어제 사용 아이템 | -15 |
| 3일 내 사용 | -8 |
| 7일 내 같은 상하의 쌍 | -20 |
| 14일 내 같은 전체 코디 | -25 |
| 7일 사용 빈도 (count/2, max 8) | -1~-8 |

| 보너스 | 점수 |
|--------|------|
| 3일 내 미사용 + 쌍 미반복 | +6 |
| 어제 미사용 | +3 |
| 휴면 아이템 1개 포함 | +12 |
| 휴면 아이템 2개 | +20 |
| 휴면 아이템 3개+ | +25 |

### 최종 점수 공식

```
final_score = style_score (base + shoe_adj + bag_adj)
            - recency_penalty
            + diversity_bonus
```

---

## 신발/가방 결정론적 매칭

LLM이 신발/가방을 안 넣었을 때 `select_best_shoe()` / `select_best_bag()` 폴백.

### 신발 매칭 점수

| 규칙 | 점수 |
|------|------|
| 구조템 | +15 |
| 연결템 | +12 |
| 베이스 | +8 |
| 상하의 둘 다와 다른 톤 | +10 |
| 전부 밝은 코디 + 어두운 신발 | +10 |
| 코디 스타일 매치 or 베이직 | +8 |
| 포멀 코디 + 러닝화 | -15 |
| neutral 색온도 | +3 |

### 가방 매칭 점수

| 규칙 | 점수 |
|------|------|
| 구조템 | +10 |
| 연결템 | +8 |
| 베이스 | +5 |
| 포인트 + 다른 포인트 존재 | -10 |
| 코디 스타일 매치 or 베이직 | +5 |
| 하의와 톤 조화 | +3 |
| neutral 색온도 | +2 |

---

## 핵심 RAG Flow: 2-Pass 구조

이미지 업로드(`POST /api/clothes/upload`) 시 실행.

```
이미지 (base64)
  ↓
PASS 1: 서술 생성 (gpt-4o-mini Vision, temp=0.3)
  → 시각적 특징 자연어 서술 (200자+)
  ↓
임베딩 검색 (OpenAI Embedding → 코사인 유사도 → 상위 5개)
  → 최고 유사도 < 0.5 → 일반 분석 폴백
  ↓
PASS 2: 정밀 분석 (gpt-4o-mini Vision, temp=0.2)
  → 레퍼런스 컨텍스트 + 원본 이미지 → 모델명 매칭
  ↓
DB 저장 (clothing + season + texture_world)
```

---

## 데이터 모델

### clothing (옷장)

| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | CHAR(36) | UUID PK |
| name | VARCHAR(100) | 아이템명 |
| category | VARCHAR(50) | 상의/하의/아우터/신발/가방 |
| color | VARCHAR(50) | 색상 |
| thickness | VARCHAR(20) | thin/medium/thick |
| image_url | LONGTEXT | base64 이미지 |
| tone | VARCHAR(20) | 밝음/중간/어두움 |
| saturation | VARCHAR(20) | 낮음/중간/높음 |
| style | VARCHAR(20) | 베이직/워크/밀리터리/포멀/스포츠 |
| weight | VARCHAR(20) | 가벼움/중간/무거움 |
| role | VARCHAR(20) | 베이스/포인트/약한포인트/연결템/구조템 |
| color_temperature | VARCHAR(20) | warm/cool/neutral |
| versatility | VARCHAR(20) | universal/flexible/situational/statement |
| statement_level | TINYINT | 1-5 (존재감) |
| formality_level | TINYINT | 1-5 (격식) |

### 연관 테이블

- **clothing_season** (clothing_id, season) — M:N, 봄/여름/가을/겨울
- **clothing_texture_world** (clothing_id, texture_world) — M:N, workwear/military/tailoring/sweat/outdoor/minimal

### clothing_reference (지식 베이스)

| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | CHAR(36) | UUID PK |
| name | VARCHAR(200) | 아이템명 |
| era | VARCHAR(100) | 시대 |
| style | VARCHAR(100) | 스타일 |
| description | TEXT | 상세 기술 설명 (임베딩 소스) |
| embedding | JSON | 1536차원 float 벡터 |

시드 데이터 13개: M-51, M-65, 정글 퍼티그, M-43, MA-1, N-1, A-2, B-15, P-41/P-47, N-3B, 셀비지 데님, 빈티지 스웻셔츠, 레트로 스니커

### outfit_recommendation_history (추천 이력)

| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | CHAR(36) | UUID PK |
| user_id | CHAR(36) | 사용자 ID |
| top_id | CHAR(36) | 상의 ID |
| bottom_id | CHAR(36) | 하의 ID |
| outer_id | CHAR(36) | 아우터 ID |
| shoes_id | CHAR(36) | 신발 ID |
| bag_id | CHAR(36) | 가방 ID |
| recommended_at | DATETIME | 추천 시각 |

인덱스: user_id+recommended_at, user_id+각 슬롯+recommended_at

---

## API 엔드포인트

### 의류 관리

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/clothes/upload` | 이미지 업로드 → RAG 분석 → 자동 등록 |
| POST | `/api/clothes` | 수동 등록 |
| GET | `/api/clothes` | 전체 목록 |
| GET | `/api/clothes/{id}` | 단일 조회 |
| PUT | `/api/clothes/{id}` | 수정 |
| DELETE | `/api/clothes/{id}` | 삭제 |

### 코디 추천

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/recommendation` | 단일 추천 (레거시) |
| POST | `/api/recommendation/multi` | **3-모드 추천** (Today/Variation/Dormant) |

### 코디 평가

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/outfit/evaluate` | 코디 조합 평가 (style_engine + LLM 설명) |

### 레퍼런스/기타

| Method | Path | 설명 |
|--------|------|------|
| GET/POST | `/api/references` | 레퍼런스 CRUD |
| GET/PUT/DELETE | `/api/references/{id}` | 레퍼런스 단일 |
| GET | `/api/health` | 헬스 체크 |
| GET/PUT | `/api/region` | 지역 설정 |
| GET | `/api/weather` | 현재 날씨 (KMA) |

---

## 프로젝트 구조

```
src/
├── main.rs                              # 진입점, AppState, 서버 초기화
├── lib.rs                               # 모듈 공개 (테스트용)
├── errors.rs                            # 에러 타입 + HTTP 매핑
├── middleware/                           # 미들웨어
├── services/
│   ├── style_engine.rs                  # 결정론적 룰 엔진 (15+ 룰)
│   ├── recommendation_service.rs        # 3-모드 선택 + 적합도 게이트
│   ├── recommendation_diversity.rs      # 리센시 페널티/보너스/휴면 감지
│   ├── openai.rs                        # Vision API, 5후보 추천, 코디 설명
│   ├── embedding.rs                     # OpenAI 임베딩, 캐시, 검색, 시드
│   └── weather.rs                       # KMA 초단기실황 API
├── routes/
│   ├── recommendation.rs                # 추천 핸들러 (/multi 포함)
│   ├── outfit.rs                        # 코디 평가
│   ├── clothes.rs                       # 의류 CRUD + 이미지 업로드
│   ├── home.rs                          # 웹 UI (3-모드 카드, 평가 화면)
│   ├── reference.rs                     # 레퍼런스 CRUD
│   ├── weather.rs / region.rs / health.rs
│   └── mod.rs
├── db/
│   ├── clothing_repo.rs                 # 의류 + season + texture_world
│   ├── recommendation_history_repo.rs   # 추천 이력 + 휴면 감지
│   ├── reference_repo.rs               # 레퍼런스
│   └── region_repo.rs                  # 지역
└── models/
    ├── outfit.rs                        # OutfitContext, EvaluationResult, Verdict, IssueCode
    ├── recommendation.rs                # OutfitCandidate, ModeRecommendation, ScoringDetail
    ├── recommendation_history.rs        # OutfitRecommendationHistory, OutfitHistorySummary
    ├── clothing.rs                      # Clothing, CreateRequest, VisionResult
    ├── reference.rs                     # ClothingReference, ReferenceMatch
    ├── weather.rs                       # CurrentWeather, KMA 응답
    └── region.rs                        # RegionSetting

tests/
└── style_engine_test.rs                 # 14개 룰 검증 테스트

migrations/
├── 20260303000001_create_clothing.sql
├── 20260303000002_create_clothing_season.sql
├── 20260303000003_create_region_setting.sql
├── 20260304000001_alter_image_url_to_longtext.sql
├── 20260305000001_create_clothing_reference.sql
├── 20260401000001_add_clothing_tags.sql
├── 20260401000002_add_style_engine_v2.sql
└── 20260405000001_create_recommendation_history.sql
```

---

## 설계 핵심 포인트

1. **판단 = 엔진, 설명 = LLM**: 스타일 품질 판단은 결정론적 룰 엔진, LLM은 자연어 설명만 담당
2. **양방향 스코어링**: 신발(-15~+10)과 가방(-8~+5)이 코디 완성도에 기여하거나 감점
3. **3-모드 하드 필터링**: 점수 공식 차이가 아닌 명시적 제외 규칙으로 모드 분리
4. **Today 적합도 게이트**: 온도/대비/구조 3축 중 2개 이상 통과 필수
5. **이력 기반 다양성**: 결정론적 리센시 패널티 + 휴면 아이템 부활 보너스
6. **1회 LLM 호출**: 5개 후보를 한 번에 생성 → 모드별 다른 후보 선택
7. **결정론적 폴백**: LLM이 신발/가방 누락 시 룰 기반 자동 매칭
8. **외부 임베딩 API**: OpenAI text-embedding-3-small 사용, 로컬 모델 상주 없음
9. **2-Pass RAG**: 시각 서술 → 임베딩 검색 → 레퍼런스 컨텍스트 정밀 분석
10. **인메모리 캐시**: 레퍼런스 임베딩 + 자동 동기화 + NULL 자동 복구
