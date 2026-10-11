-- 기존 레퍼런스 13개에 카테고리와 장르를 채운다.
--
-- `20261011000001` 이 컬럼을 추가했지만 13행은 NULL 이다. 카테고리 선필터는 이 값이
-- 없으면 걸리지 않으므로, 선필터를 켜기 전에 먼저 채워야 한다.
--
-- ─── 어휘 ───
--
-- `category` 는 한국어다. `clothing.category` 가 남녀 모두 한국어(상의/하의/아우터/
-- 신발/가방)로 일관되기 때문이고, Vision 이 돌려주는 값도 같은 어휘다.
--
-- `subcategory` 는 영어다. 같은 축인 `clothing.sub_category` 가 **성별로 갈려 있다** —
-- 남성은 `field_jacket` · `bomber` · `denim`, 여성은 `필드자켓` · `봄버자켓` ·
-- `데님팬츠` 다. 둘 중 영어를 쓰는 이유는 ① 이 13개가 남성 아이템이고 그쪽 어휘가
-- 영어이며, ② 영어 쪽이 `20260507000005_add_missing_item_metadata.sql` 의 COMMENT 로
-- 선언된 계약이고, ③ 여성 쪽 한국어 값은 정규화가 안 돼 있다(`가디건` 과 `카디건`,
-- `트랙재킷` 과 `track_jacket` 이 함께 있다).
-- 이 불일치 자체는 여기서 건드리지 않는다.
--
-- ─── 장르 ───
--
-- **아래 장르 배정은 작성자 판단이고 검수받지 않았다.** 확신이 서는 것만 넣고 애매한
-- 것은 비웠다. 특히 밀리터리 아우터를 `street` 에 넣지 않았다 — M-65 가 스트리트에서
-- 흔히 쓰이는 것은 사실이지만, 그 근거는 이 레퍼런스의 서술(1965년 미군 야전상의)이
-- 아니라 현대의 착용 관행이고, 레퍼런스 검색은 서술로 이뤄진다.
--
-- 결과적으로 13개는 사실상 `amekaji` 한 장르만 덮는다. 그게 현재 커버리지의 실상이다.

UPDATE clothing_reference
SET category = CASE
    WHEN name IN (
      'M-51 피쉬테일 파카', 'M-65 필드 자켓', '정글 퍼티그 자켓', 'M-43 필드 자켓',
      'MA-1 봄버 자켓', 'N-1 데크 자켓', 'A-2 플라이트 자켓', 'B-15 플라이트 자켓',
      'P-41/P-47 HBT 유틸리티 자켓', 'N-3B 스노클 파카'
    ) THEN '아우터'
    WHEN name = '셀비지 데님 진'   THEN '하의'
    WHEN name = '빈티지 스웻셔츠'  THEN '상의'
    WHEN name = '레트로 스니커'    THEN '신발'
    ELSE NULL
  END,
  subcategory = CASE
    WHEN name IN ('M-51 피쉬테일 파카', 'N-3B 스노클 파카')        THEN 'parka'
    WHEN name IN ('M-65 필드 자켓', 'M-43 필드 자켓', '정글 퍼티그 자켓') THEN 'field_jacket'
    WHEN name = 'MA-1 봄버 자켓'                                  THEN 'bomber'
    WHEN name = 'N-1 데크 자켓'                                   THEN 'deck'
    WHEN name IN ('A-2 플라이트 자켓', 'B-15 플라이트 자켓')        THEN 'blouson'
    WHEN name = 'P-41/P-47 HBT 유틸리티 자켓'                     THEN 'jacket'
    WHEN name = '셀비지 데님 진'                                   THEN 'denim'
    WHEN name = '빈티지 스웻셔츠'                                  THEN 'sweat'
    WHEN name = '레트로 스니커'                                    THEN 'sneaker'
    ELSE NULL
  END
WHERE category IS NULL;

INSERT IGNORE INTO clothing_reference_genre (reference_id, style_genre)
SELECT r.id, g.genre
FROM clothing_reference r
JOIN (
  SELECT 'M-51 피쉬테일 파카' AS name, 'amekaji' AS genre
  UNION ALL SELECT 'M-65 필드 자켓', 'amekaji'
  UNION ALL SELECT '정글 퍼티그 자켓', 'amekaji'
  UNION ALL SELECT 'M-43 필드 자켓', 'amekaji'
  UNION ALL SELECT 'MA-1 봄버 자켓', 'amekaji'
  UNION ALL SELECT 'N-1 데크 자켓', 'amekaji'
  UNION ALL SELECT 'A-2 플라이트 자켓', 'amekaji'
  UNION ALL SELECT 'B-15 플라이트 자켓', 'amekaji'
  UNION ALL SELECT 'N-3B 스노클 파카', 'amekaji'
  UNION ALL SELECT 'P-41/P-47 HBT 유틸리티 자켓', 'amekaji'
  UNION ALL SELECT 'P-41/P-47 HBT 유틸리티 자켓', 'workwear'
  UNION ALL SELECT '셀비지 데님 진', 'amekaji'
  UNION ALL SELECT '셀비지 데님 진', 'workwear'
  UNION ALL SELECT '빈티지 스웻셔츠', 'amekaji'
  UNION ALL SELECT '레트로 스니커', 'amekaji'
  UNION ALL SELECT '레트로 스니커', 'sporty_casual'
) g ON g.name = r.name;
