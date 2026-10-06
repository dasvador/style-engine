-- 매니시에 테일러링을 더한다.
--
-- 이 무드의 정의는 "남성복에서 가져온 테일러링과 넓은 어깨, 와이드 슬랙스" 인데,
-- 실제 아이템에는 블레이저도 슬랙스도 한 벌이 없었다. 럭비 티·카고팬츠·데님자켓·
-- 캔버스 토트뿐이라 생성 결과가 매니시가 아니라 보이시 캐주얼로 나왔다.
-- 무드 여덟 개를 같은 조건으로 생성해 비교하다 드러났다.
--
-- 옷을 지우지 않고 더한다. 재실행에 안전하다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, material_primary, sub_category, texture_keywords)
VALUES
  -- ─── 아우터: 넓은 어깨를 만드는 축 ───
  ('0ffd0a00-0000-0000-0000-000000000000', '차콜 울 오버사이즈 블레이저', '아우터', 'female', 'mannish',
   'charcoal', 'medium', '어두움', '낮음', '포멀', '중간', '구조템', 3, 'wool', '블레이저',
   'wool flannel, structured shoulder, oversized blazer'),
  ('0ffd0a00-0000-0000-0000-000000000001', '블랙 더블브레스티드 테일러드 자켓', '아우터', 'female', 'mannish',
   'black', 'medium', '어두움', '낮음', '포멀', '중간', '구조템', 4, 'wool', '자켓',
   'wool, double breasted, sharp lapel, tailored'),

  -- ─── 하의: 와이드 슬랙스 ───
  ('0ffd0a00-0000-0000-0000-000000000002', '차콜 울 와이드 슬랙스', '하의', 'female', 'mannish',
   'charcoal', 'medium', '어두움', '낮음', '포멀', '중간', '베이스', 3, 'wool', '슬랙스',
   'wool, wide leg, pressed crease'),
  ('0ffd0a00-0000-0000-0000-000000000003', '블랙 하이웨이스트 핀턱 슬랙스', '하의', 'female', 'mannish',
   'black', 'medium', '어두움', '낮음', '포멀', '중간', '베이스', 3, 'polyester', '슬랙스',
   'pin tuck, high waist, wide leg'),

  -- ─── 상의: 테일러링과 맞물리는 단정한 셔츠와 니트 ───
  ('0ffd0a00-0000-0000-0000-000000000004', '화이트 코튼 포플린 오버핏 셔츠', '상의', 'female', 'mannish',
   'white', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 2, 'cotton', '셔츠',
   'cotton poplin, dropped shoulder, oversized shirt'),
  ('0ffd0a00-0000-0000-0000-000000000005', '차콜 메리노울 크루넥 니트', '상의', 'female', 'mannish',
   'charcoal', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 2, 'wool', '니트',
   'merino wool, fine gauge, plain crewneck'),

  -- ─── 신발·가방: 운동화 일색을 끊는다 ───
  ('0ffd0a00-0000-0000-0000-000000000006', '블랙 레더 더비 슈즈', '신발', 'female', 'mannish',
   'black', 'medium', '어두움', '낮음', '포멀', '중간', '구조템', 3, 'leather', '더비슈즈',
   'polished calf leather, welt stitching, derby'),
  ('0ffd0a00-0000-0000-0000-000000000007', '블랙 레더 스트럭처드 토트백', '가방', 'female', 'mannish',
   'black', 'medium', '어두움', '낮음', '베이직', '중간', '약한포인트', 3, 'leather', '토트백',
   'leather, structured tote, clean edge finishing')
ON DUPLICATE KEY UPDATE id = clothing.id;

-- 카고팬츠는 무드 설명에서 스트리트 쪽에 명시돼 있다("오버사이즈 실루엣과 그래픽,
-- 데님과 카고"). 매니시에 남겨 두면 테일러링을 더해도 결과가 계속 캐주얼로 샌다.
UPDATE clothing SET style_mood = 'street'
  WHERE gender = 'female' AND style_mood = 'mannish' AND name = '카키 코튼 카고팬츠';

-- 계절은 `20260910000009` 의 규칙을 그대로 적용한다.
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, s.season
FROM clothing c
CROSS JOIN (
  SELECT '봄' AS season UNION ALL SELECT '여름' UNION ALL SELECT '가을' UNION ALL SELECT '겨울'
) s
WHERE c.id LIKE '0ffd0a00-%'
  AND FIND_IN_SET(
    s.season,
    CASE
      WHEN c.sub_category IN ('더비슈즈', '토트백') THEN '봄,여름,가을,겨울'
      WHEN c.material_primary = 'wool' THEN '가을,겨울'
      ELSE '봄,가을'
    END
  ) > 0;
