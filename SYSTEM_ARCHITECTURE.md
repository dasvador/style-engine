# Military Vintage Clothing Recognition System

밀리터리 빈티지 의류 이미지 인식 시스템 아키텍처 문서

## Tech Stack

| 구분 | 기술 |
|------|------|
| Framework | Axum (Rust) + Tokio |
| Database | MySQL (sqlx) |
| Vision AI | OpenAI gpt-4o-mini |
| Embedding | fastembed (multilingual-e5-small, 384차원, ONNX 로컬) |
| Weather | Open-Meteo API |

---

## 핵심 RAG Flow: 2-Pass 구조

이미지 업로드(`POST /api/clothes/upload`) 시 아래 파이프라인이 실행된다.

```
┌─────────────────────────────────────────────────────────┐
│                    이미지 업로드                          │
│               (base64 data URL)                         │
└──────────────────────┬──────────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────────┐
│  PASS 1: 서술 생성 (analyze_clothing_pass1)              │
│                                                         │
│  • OpenAI gpt-4o-mini Vision API (temp=0.3)             │
│  • 이미지의 시각적 특징을 자연어로 서술                      │
│    → 칼라 형태, 원단, 무게감, 색상, 디테일 등               │
│  • 출력: { "description": "200자 이상 텍스트" }           │
└──────────────────────┬──────────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────────┐
│  임베딩 검색 (EmbeddingService::search)                  │
│                                                         │
│  1. Pass1 서술 텍스트 → fastembed → 384차원 벡터          │
│  2. 인메모리 캐시의 레퍼런스 임베딩들과 코사인 유사도 계산    │
│  3. 유사도 내림차순 정렬 → 상위 5개 반환                    │
│  4. 최고 유사도 < 0.5 → 일반 분석 폴백                     │
└──────────────────────┬──────────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────────┐
│  PASS 2: 정밀 분석 (analyze_clothing_pass2)              │
│                                                         │
│  • OpenAI gpt-4o-mini Vision API (temp=0.2)             │
│  • 입력: 원본 이미지 + Pass1 서술 + 검색된 레퍼런스 5건    │
│  • 레퍼런스 컨텍스트를 시스템 프롬프트에 포함               │
│  • AI가 레퍼런스와 비교하여 정확한 모델명 매칭              │
│  • 출력:                                                 │
│    {                                                     │
│      "is_clothing": true,                                │
│      "name": "M-65 필드 자켓",                            │
│      "category": "아우터",                                │
│      "color": "올리브 그린",                               │
│      "thickness": "thick",                               │
│      "seasons": ["봄", "가을"]                            │
│    }                                                     │
└──────────────────────┬──────────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────────┐
│  DB 저장                                                 │
│  • clothing 테이블에 INSERT                               │
│  • clothing_season 테이블에 시즌 INSERT                   │
│  • ClothingResponse 반환                                 │
└─────────────────────────────────────────────────────────┘
```

### 폴백 경로

유사도 최고값이 0.5 미만이면 `analyze_clothing_image()` (직접 분석)으로 폴백한다.
이 경로는 레퍼런스 컨텍스트 없이, 30개 이상의 브랜드 지식이 포함된 상세 시스템 프롬프트로 분석한다.

---

## 임베딩 시스템

### 모델

- **multilingual-e5-small** (ONNX, 로컬 실행)
- 384차원 벡터 출력
- 한국어/영어 멀티링구얼 지원
- 첫 실행 시 ~80MB 모델 자동 다운로드 → `.fastembed_cache/`에 캐시

### 운영 방식

```
서버 시작
  ├─ 모델 초기화 (EmbeddingService::new)
  ├─ DB 마이그레이션 실행
  ├─ 시드 데이터 삽입 (clothing_reference 비어있을 때)
  └─ 캐시 로딩 (load_cache)
       ├─ DB에서 전체 레퍼런스 조회
       ├─ embedding=NULL인 항목 → 자동 생성 후 DB 업데이트
       └─ 인메모리 HashMap에 적재 (RwLock)
```

### 검색 과정

```rust
// 1. 쿼리 텍스트를 임베딩
let query_vec = embed_text(query);

// 2. 캐시된 모든 레퍼런스와 코사인 유사도 계산
for ref in cache {
    similarity = dot(query_vec, ref.embedding) / (norm(a) * norm(b));
}

// 3. 유사도 내림차순 정렬 → 상위 5개 반환
```

---

## 데이터 모델

### clothing_reference (지식 베이스)

| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | CHAR(36) | UUID PK |
| name | VARCHAR(200) | 아이템명 (예: "M-51 피쉬테일 파카") |
| era | VARCHAR(100) | 시대 (예: "1950s 한국전쟁") |
| style | VARCHAR(100) | 스타일 (예: "밀리터리") |
| description | TEXT | 상세 기술 설명 (임베딩 소스) |
| embedding | JSON | 384차원 float 벡터 |

### 시드 데이터 (13개 항목)

밀리터리: M-51 파카, M-65, 정글 퍼티그, M-43, MA-1, N-1, A-2, B-15, P-41/P-47, N-3B
캐주얼: 셀비지 데님, 빈티지 스웻셔츠, 레트로 스니커

각 항목에 한국어로 작성된 상세 기술 설명이 포함되어 있다. (칼라 형태, 원단, 포켓 구조, 라이닝 등)

---

## API 엔드포인트

### 의류 관리

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/clothes/upload` | **이미지 업로드 → RAG 분석 → 자동 등록** |
| POST | `/api/clothes` | 수동 등록 |
| GET | `/api/clothes` | 전체 목록 |
| GET | `/api/clothes/{id}` | 단일 조회 |
| PUT | `/api/clothes/{id}` | 수정 |
| DELETE | `/api/clothes/{id}` | 삭제 |

### 레퍼런스 관리

| Method | Path | 설명 |
|--------|------|------|
| GET | `/api/references` | 전체 목록 |
| POST | `/api/references` | 추가 (자동 임베딩 + 캐시 리로드) |
| GET | `/api/references/{id}` | 단일 조회 |
| PUT | `/api/references/{id}` | 수정 (재임베딩 + 캐시 리로드) |
| DELETE | `/api/references/{id}` | 삭제 (캐시 리로드) |

### 코디 추천

| Method | Path | 설명 |
|--------|------|------|
| POST | `/api/recommendation` | 날씨 기반 AI 코디 추천 |

추천 흐름: 지역 조회 → 날씨 API 호출 → 사용자 옷장 조회 → OpenAI 코디 생성

### 기타

| Method | Path | 설명 |
|--------|------|------|
| GET | `/api/health` | 헬스 체크 + DB ping |
| PUT | `/api/region` | 지역 설정 (위도/경도) |
| GET | `/api/region` | 지역 조회 |
| GET | `/api/weather` | 현재 날씨 조회 |

---

## 프로젝트 구조

```
src/
├── main.rs                    # 진입점, 서버 초기화, 상태 구성
├── errors.rs                  # 에러 타입 + HTTP 매핑
├── services/
│   ├── embedding.rs           # fastembed 래퍼, 캐시, 검색, 시드
│   ├── openai.rs              # Vision API (Pass1/Pass2), 코디 추천
│   └── weather.rs             # Open-Meteo API
├── routes/
│   ├── mod.rs                 # 라우터 구성
│   ├── clothes.rs             # 의류 CRUD + 이미지 업로드
│   ├── reference.rs           # 레퍼런스 CRUD
│   ├── recommendation.rs      # 코디 추천
│   ├── weather.rs             # 날씨
│   ├── region.rs              # 지역 설정
│   ├── health.rs              # 헬스 체크
│   └── home.rs                # 웹 UI (인라인 HTML/JS)
├── db/
│   ├── clothing_repo.rs       # 의류 DB 접근
│   ├── reference_repo.rs      # 레퍼런스 DB 접근
│   └── region_repo.rs         # 지역 DB 접근
└── models/
    ├── clothing.rs            # Clothing, VisionAnalysisResult, Pass1Result
    ├── reference.rs           # ClothingReference, ReferenceMatch
    ├── recommendation.rs      # RecommendationRequest, AiRecommendation
    ├── weather.rs             # CurrentWeather, OpenMeteoResponse
    └── region.rs              # RegionSetting

migrations/
├── 20260303000001_create_clothing.sql
├── 20260303000002_create_clothing_season.sql
├── 20260303000003_create_region_setting.sql
├── 20260304000001_alter_image_url_to_longtext.sql
└── 20260305000001_create_clothing_reference.sql
```

---

## 설계 핵심 포인트

1. **로컬 임베딩**: fastembed(ONNX) 사용으로 API 비용 없이 한국어 임베딩 지원
2. **인메모리 캐시**: 레퍼런스 임베딩을 메모리에 적재하여 검색 시 DB 조회 불필요
3. **2-Pass 전략**: Pass1에서 고품질 텍스트를 먼저 뽑고, 이를 임베딩하여 유사 레퍼런스를 검색한 뒤 Pass2에서 컨텍스트와 함께 정밀 분석
4. **유사도 임계값(0.5)**: 일치도가 낮으면 레퍼런스에 억지로 매칭하지 않고 일반 분석으로 폴백
5. **자동 캐시 동기화**: 레퍼런스 CRUD 시 임베딩 캐시 자동 리로드
6. **NULL 임베딩 자동 복구**: 서버 시작 시 embedding=NULL인 항목 자동 생성
