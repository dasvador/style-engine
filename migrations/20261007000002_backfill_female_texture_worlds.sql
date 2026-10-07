-- 여성 의류의 텍스처 월드를 채우고, 표준 밖 값 두 개를 정리한다.
--
-- `clothing_texture_world` 에 여성 행은 하나도 없었다(남성은 154벌 중 140벌 보유).
-- 이 표가 비어 있으면 아래 규칙들이 여성 추천에서 전혀 작동하지 않는다:
--   - `sweat` + `tailoring` 충돌 (스웨트셔츠에 테일러드 블레이저)
--   - `outdoor` + `tailoring` 약한 충돌
--   - `military` + `tailoring` / `workwear` + `military` 밸런싱 가점
--   - 테마 월드를 아우터와 하의가 공유할 때의 감점
--   - 하드필터의 weak anchor 구제(`workwear` 또는 `minimal` 을 본다)
--
-- ─── 분류 근거 ───
--
-- 이 표는 소재 매핑이 아니라 **미학 월드 태그**다. 그래서 값을 지어내지 않고 남성
-- 140벌의 기존 배정에서 역산했다. 거기서 읽히는 규칙은 두 갈래다.
--
--   (1) 품목·스타일이 월드를 하나 준다.
--       밀리터리 → military, 워크 → workwear, 베이직 → minimal,
--       blazer·coat → tailoring, slacks 는 포멀일 때만 tailoring
--       (`그레이지 울 밴딩 팬츠` 는 같은 slacks 인데 베이직이라 minimal 이다),
--       스웨트셔츠류 → sweat.
--   (2) 소재가 그 자체로 월드인 경우 하나 더 붙는다.
--       denim, leather(suede 포함), linen, canvas, nylon(cordura 포함),
--       corduroy → workwear.
--
-- **스포츠 스타일은 월드를 받지 않는다.** 남성 스포츠 15벌 중 14벌이 비어 있고,
-- 남성 미보유 14벌이 정확히 그 집합이다. 의도된 공백으로 보고 그대로 따른다.
--
-- 남성 배정은 완벽히 일관되지는 않다 — `샌드 리넨 셔츠`는 minimal 인데 `크림 리넨
-- 셔츠`는 linen 이고, `크림 치노 팬츠`는 minimal 인데 `올리브 치노 팬츠`는
-- military,workwear 다. 그래서 위 두 갈래만 규칙으로 삼고 나머지 편차는 따르지 않았다.
--
-- ─── 일부러 비워 두는 것 ───
--
-- 새틴·쉬폰·오간자·레이스·트위드·크로셰 같은 소재와 자수·셔링 같은 장식은 현재
-- 어휘에 들어갈 월드가 없다. 로맨틱·보헤미안 쪽 33벌이 여기 해당하고 NULL 로 남는다.
-- `minimal` 을 기본값으로 채우지 않는 이유: `minimal` 은 "강한 세계관이 없는 기본기"
-- 라는 뜻이고 하드필터의 weak anchor 구제가 이 값을 본다. 자수 오간자 블라우스를
-- minimal 로 적으면 구제하지 않아야 할 착장을 구제한다. 월드를 새로 만들지 여부는
-- 충돌 표(무엇이 무엇과 부딪히는지)까지 같이 정해야 하는 판단이라 남겨 둔다.
--
-- 재실행해도 결과가 같다 (INSERT IGNORE + 복합 PK).

-- ─── 1. 표준 밖 값 정리 ───
--
-- `올리브 나일론 MA-1 봄버 자켓` 한 벌이 `나일론` · `매끄러운` 을 들고 있다. 같은
-- 조건(nylon + 밀리터리)의 다른 MA-1 은 `military` · `nylon` 이므로 분류 판단이
-- 아니라 입력 실수다. 이 두 값은 어느 규칙에도 걸리지 않아 조용히 살아 있었다.
DELETE FROM clothing_texture_world WHERE texture_world IN ('나일론', '매끄러운');

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT c.id, w.world
FROM clothing c
CROSS JOIN (SELECT 'military' AS world UNION ALL SELECT 'nylon') w
WHERE c.gender = 'male'
  AND c.material_primary = 'nylon'
  AND c.style = '밀리터리';

-- ─── 2. 여성 의류 백필 ───

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT c.id, w.world
FROM clothing c
CROSS JOIN (
  SELECT 'military' AS world
  UNION ALL SELECT 'workwear'
  UNION ALL SELECT 'sweat'
  UNION ALL SELECT 'tailoring'
  UNION ALL SELECT 'outdoor'
  UNION ALL SELECT 'minimal'
  UNION ALL SELECT 'denim'
  UNION ALL SELECT 'leather'
  UNION ALL SELECT 'linen'
  UNION ALL SELECT 'canvas'
  UNION ALL SELECT 'nylon'
) w
WHERE c.gender = 'female'
  AND FIND_IN_SET(
    w.world,
    CONCAT_WS(
      ',',
      -- (1) 품목·스타일 월드. 위에서부터 먼저 걸리는 것이 이긴다.
      CASE
        WHEN c.style = '스포츠' THEN NULL
        WHEN c.style = '밀리터리'
          OR c.sub_category IN ('필드자켓', '카고팬츠') THEN 'military'
        WHEN c.style = '워크'
          OR c.sub_category IN ('트러커자켓', '유틸리티자켓') THEN 'workwear'
        WHEN c.sub_category IN ('맨투맨', '후드티', '후드집업')
          OR c.name LIKE '%스웨트%' THEN 'sweat'
        WHEN c.sub_category IN ('블레이저', '코트')
          OR c.name LIKE '%테일러드%'
          OR (c.sub_category IN ('슬랙스', '와이드팬츠') AND c.style = '포멀') THEN 'tailoring'
        WHEN c.sub_category = '윈드브레이커' THEN 'outdoor'
        WHEN c.style = '베이직' THEN 'minimal'
        ELSE NULL
      END,
      -- (2) 소재가 그 자체로 월드인 경우.
      CASE
        WHEN c.style = '스포츠' THEN NULL
        WHEN c.material_primary = 'denim' THEN 'denim'
        WHEN c.material_primary IN ('leather', 'suede') THEN 'leather'
        WHEN c.material_primary = 'linen' THEN 'linen'
        WHEN c.material_primary IN ('canvas', 'raffia') THEN 'canvas'
        WHEN c.material_primary IN ('nylon', 'cordura') THEN 'nylon'
        WHEN c.material_primary = 'corduroy' THEN 'workwear'
        ELSE NULL
      END
    )
  );
