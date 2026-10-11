-- 여성 의류의 무게와 격식을 채운다.
--
-- 여성 176벌 중 141벌이 `weight` · `formality_level` 이 NULL 이었다. 이 두 값은 실제
-- 추천 경로가 읽는다 — 상황별 격식 판정(`style_engine::rule_formality_situation`),
-- 후보 선별(`shortlist`), 순위 조정(`recommendation_service`). 격식이 비면 상황 판정이
-- 통째로 건너뛰어져서, 채우기 전 측정에서 출근에 티셔츠+청바지가 86점 "Good",
-- 데이트에 맨투맨+조거가 89점 "Great" 였다.
--
-- ─── 무게: 두께를 따른다 ───
--
-- 값이 있는 194벌 중 169벌(87%)이 thin→가벼움, medium→중간, thick→무거움 이다.
-- 나머지 25벌의 예외는 품목이나 소재로 설명되지 않는다 — 같은 티셔츠 12벌 중 4벌만
-- 가벼움, 같은 가죽 13벌 중 2벌만 무거움이다. 사진을 보고 한 벌씩 판단한 결과로 보고,
-- 그 판단은 이미지가 없는 여성 아이템에서 재현할 수 없으므로 따르지 않는다.
--
-- ─── 격식: 규칙과 남성 데이터의 척도를 따른다 ───
--
-- 상황 판정 규칙은 출근이면 평균 격식 3 이상, 데이트 2~4, 캐주얼 1~2.5 를 요구한다.
-- 남성 데이터는 이 척도로 매겨져 있다 — 티·스웻·운동복 1, 셔츠·니트·치노·데님 2,
-- 로퍼·코트·부츠 3, 슬랙스·블레이저·더비 4.
--
-- 여성 기존 35벌은 이보다 한 단계 낮다(슬랙스 3, 로퍼 2). 그 척도로 채우면 평범한
-- 출근 착장(블라우스+슬랙스+로퍼+토트)이 평균 2.25 로 "너무 캐주얼" 감점을 받는다.
-- 대표 착장 10개로 미리 계산한 결과 기존 여성 척도는 7/10, 아래 척도는 10/10 이
-- 상식과 맞았다. 그래서 규칙의 척도를 따른다.
--
-- 기존 35벌은 건드리지 않는다. 이 마이그레이션은 빈 값만 채운다.
--
-- 판정 순서: 스포츠 스타일은 품목과 관계없이 1. 그 다음 품목별 기본값, 마지막으로
-- 포멀 스타일이면 한 단계 올린다(최대 4 — 기존 데이터에 5는 없다). 품목이 목록에
-- 없으면 CASE 가 NULL 이 되고 NULL + 1 도 NULL 이라 비운 채로 남는다.
--
-- 값이 이미 있는 행은 건드리지 않으므로 여러 번 실행해도 결과가 같다.

UPDATE clothing
SET weight = CASE thickness
    WHEN 'thin'   THEN '가벼움'
    WHEN 'medium' THEN '중간'
    WHEN 'thick'  THEN '무거움'
    ELSE NULL
  END
WHERE gender = 'female'
  AND weight IS NULL;

UPDATE clothing
SET formality_level = LEAST(
  4,
  CASE
      -- 스포츠 스타일은 품목과 관계없이 가장 캐주얼하다(남성 스포츠 17벌 평균 1.18).
      WHEN style = '스포츠' THEN 1

      -- ─── 상의 ───
      WHEN category = '상의' AND sub_category IN (
        '티셔츠', '반팔티셔츠', '긴팔티셔츠', '크롭티셔츠', '크롭탑', '탱크탑',
        '브라탑', '맨투맨', '후드티', '롱슬리브'
      ) THEN 1
      WHEN category = '상의' AND sub_category IN (
        '니트', '니트탑', '하프집업니트', '셔츠', '폴로셔츠', '탑', '오프숄더블라우스'
      ) THEN 2
      WHEN category = '상의' AND sub_category = '블라우스' THEN 3

      -- ─── 하의 ───
      WHEN category = '하의' AND sub_category IN (
        '레깅스', '숏레깅스', '숏팬츠', '조거팬츠', '카고팬츠'
      ) THEN 1
      -- 데님은 이름으로 가른다 — 와이드팬츠에 데님과 트라우저가 섞여 있다.
      WHEN category = '하의' AND (
        sub_category IN ('데님팬츠', '플레어진')
        OR name LIKE '%데님%'
      ) THEN 2
      WHEN category = '하의' AND sub_category IN (
        '치노팬츠', '맥시스커트', '원피스'
      ) THEN 2
      WHEN category = '하의' AND sub_category IN (
        '스커트', '롱스커트', '와이드팬츠'
      ) THEN 3
      WHEN category = '하의' AND sub_category = '슬랙스' THEN 4

      -- ─── 아우터 ───
      WHEN category = '아우터' AND sub_category IN (
        '후드집업', '윈드브레이커', '트랙재킷'
      ) THEN 1
      WHEN category = '아우터' AND sub_category IN (
        '데님자켓', '봄버자켓', '필드자켓', '트러커자켓', '셔츠자켓', '가디건',
        '카디건', '크로셰가디건', '프린지자켓', '라이더자켓', '유틸리티자켓'
      ) THEN 2
      WHEN category = '아우터' AND sub_category = '블레이저' THEN 4
      WHEN category = '아우터' AND sub_category = '자켓'
        AND name LIKE '%테일러드%' THEN 4
      WHEN category = '아우터' AND sub_category IN (
        '코트', '트렌치코트', '자켓'
      ) THEN 3

      -- ─── 신발 ───
      WHEN category = '신발' AND sub_category IN (
        '스니커즈', '러닝화', '스포츠샌들', '에스파드리유', '글래디에이터샌들'
      ) THEN 1
      WHEN category = '신발' AND sub_category = '뮬' THEN 2
      WHEN category = '신발' AND sub_category IN (
        '로퍼', '플랫슈즈', '메리제인', '발레리나', '웨스턴부츠', '앵클부츠',
        '첼시부츠', '더비슈즈'
      ) THEN 3
      WHEN category = '신발' AND sub_category IN ('힐', '슬링백') THEN 4

      -- ─── 가방 ─── (가방의 '슬링백' 은 몸에 메는 슬링 백이고 신발의 슬링백과 다르다)
      WHEN category = '가방' AND sub_category IN (
        '백팩', '슬링백', '웨이스트백'
      ) THEN 1
      WHEN category = '가방' AND sub_category IN (
        '크로스백', '메신저백', '버킷백', '호보백'
      ) THEN 2
      WHEN category = '가방' AND sub_category IN (
        '숄더백', '토트백', '사첼백'
      ) THEN 3

      -- 목록에 없는 품목은 비워 둔다. 기본값을 넣으면 새 품목이 조용히 2가 된다.
      ELSE NULL
    END + IF(style = '포멀', 1, 0)
)
WHERE gender = 'female'
  AND formality_level IS NULL;
