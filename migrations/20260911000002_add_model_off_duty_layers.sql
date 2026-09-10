-- 모델 오프듀티에 중간 두께 상의와 아우터를 더한다.
--
-- `20260911000001` 로 가방·신발을 넓혔더니 착장이 5회 중 4회 서로 달라졌지만,
-- 상의와 하의는 여전히 몰렸다. 원인은 다양성이 아니라 온도 게이트였다.
--
-- `recommendation_service::evaluate_today_fitness` 는 13~18도 구간에서 얇은 상의를
-- 아우터 없이 입은 후보에 감점을 주고 soft fail 처리한다. 그런데 이 장르의 상의
-- 7벌 중 5벌이 `thin` 이고 아우터는 2벌뿐이었다. 그래서 봄가을 기온에서 자격을
-- 통과하는 상의가 사실상 `오트밀 코튼 니트 크루넥` 과 `차콜 코튼 후드 스웨트셔츠`
-- 둘뿐이었고, 추천 이력에 그 둘만 반복해서 나타났다.
--
-- 즉 반복 감점은 정상 동작하고 있었다. 감점이 밀어내도 갈 곳이 없었을 뿐이다.
--
-- 그래서 겹쳐 입을 수 있는 중간 두께 상의 둘과 얇은 티셔츠를 살릴 아우터 둘을
-- 더한다. 데모 옷장에만 넣으며, 재실행에 안전하다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, material_primary, sub_category, texture_keywords)
VALUES
  -- 중간 두께 상의 — 아우터 없이도 이 기온을 통과한다.
  ('0ffd0900-0000-0000-0000-000000000000', '화이트 코튼 오버사이즈 셔츠', '상의', 'female', 'model_off_duty',
   'white', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 2, 'cotton', '셔츠',
   'cotton poplin, oversized, dropped shoulder'),
  ('0ffd0900-0000-0000-0000-000000000001', '그레이 코튼 긴팔 티셔츠', '상의', 'female', 'model_off_duty',
   'gray', 'medium', '중간', '낮음', '베이직', '중간', '베이스', 1, 'cotton', '긴팔티셔츠',
   'heavy cotton jersey, long sleeve'),

  -- 아우터 — 얇은 티셔츠를 겹쳐 입을 수 있게 해 후보에서 탈락하지 않도록.
  ('0ffd0900-0000-0000-0000-000000000002', '카키 코튼 유틸리티 자켓', '아우터', 'female', 'model_off_duty',
   'khaki', 'medium', '중간', '낮음', '베이직', '중간', '연결템', 1, 'cotton', '유틸리티자켓',
   'cotton twill, patch pockets, relaxed'),
  ('0ffd0900-0000-0000-0000-000000000003', '그레이 코튼 후드 집업', '아우터', 'female', 'model_off_duty',
   'gray', 'medium', '중간', '낮음', '베이직', '중간', '연결템', 1, 'cotton', '후드집업',
   'french terry, zip hoodie')
ON DUPLICATE KEY UPDATE id = clothing.id;

-- 계절은 `20260910000009` 의 규칙을 그대로 적용한다.
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, s.season
FROM clothing c
CROSS JOIN (
  SELECT '봄' AS season UNION ALL SELECT '여름' UNION ALL SELECT '가을' UNION ALL SELECT '겨울'
) s
WHERE c.id LIKE '0ffd0900-%'
  AND FIND_IN_SET(
    s.season,
    CASE
      WHEN c.sub_category = '후드집업' THEN '봄,가을,겨울'
      ELSE '봄,가을'
    END
  ) > 0;
