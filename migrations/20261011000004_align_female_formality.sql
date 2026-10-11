-- 여성 의류 기존 35벌의 격식을 20261011000003 과 같은 척도로 맞춘다.
--
-- 20261011000003 은 빈 값 141벌만 채웠다. 값이 이미 있던 35벌은 그 척도보다 한 단계
-- 낮게 매겨져 있었다 — 슬랙스·블레이저·더비 3, 로퍼·부츠·숄더백 2, 데님 1. 17벌이
-- 다르고 차이는 모두 정확히 1 이다. 나머지 18벌은 이미 같다.
--
-- 섞여 있으면 착장 판정이 어긋난다. 적용 전 측정(`/api/outfit/evaluate`, 실제 아이템):
--   출근 · 블레이저+블라우스+울 슬랙스+더비+토트    83점, "너무 캐주얼" (평균 2.8)
--   출근 · 블라우스+핀턱 슬랙스+로퍼+미니 숄더      79점, "너무 캐주얼" (평균 2.8)
--   데이트 · 니트+스트레이트 데님+앵클부츠+호보      72점, "너무 캐주얼" (평균 1.5)
-- 셋 다 상황에 맞는 착장이다. 같은 슬랙스가 3 이냐 4 냐에 따라 판정이 갈린다.
--
-- 아래 식은 20261011000003 의 격식 식을 그대로 옮긴 것이다. 두 곳의 규칙이 달라지면
-- 다시 섞이므로, 고칠 때는 둘을 함께 고쳐야 한다(마이그레이션은 한 번 적용되면
-- 바꾸지 않으므로, 실제로는 새 마이그레이션으로 둘 다 다시 맞춘다).
--
-- 규칙 밖 품목(식이 NULL)은 건드리지 않는다. 이미 같은 값이면 바꾸지 않으므로 여러 번
-- 실행해도 결과가 같다. 다만 사용자가 나중에 여성 아이템의 격식을 직접 고친 뒤 이
-- 파일을 손으로 다시 돌리면 그 수정을 덮어쓴다 — sqlx 는 한 번만 적용한다.

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
  AND (LEAST(
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
  )) IS NOT NULL
  AND formality_level <> (LEAST(
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
  ));
