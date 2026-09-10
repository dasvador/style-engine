-- 모델 오프듀티 데모 옷장을 넓힌다.
--
-- 이 장르는 추천이 계속 같은 착장으로 나왔다. 이력을 보면 네 번 중 세 번이
-- 완전히 동일했고, 상의는 네 번 모두 같은 니트였다.
--
-- 추천 경로 자체는 정상이다. `/multi` 는 shortlist 를 만들 때 최근 추천 아이템에
-- 감점을 주고(`shortlist::ShortlistContext::recent_item_ids`), 후보를 받은 뒤에도
-- `recommendation_service::rerank_candidates_with_history` 로 반복 감점과 다양성
-- 보너스를 다시 매긴다. 문제는 감점이 고를 대상이 없다는 것이었다.
--
-- `20260910000007` 이 이 장르를 만들 때 "후보가 0건인 칩" 을 없앨 만큼만 넣었다.
-- 그 결과 가방이 한 개뿐이라 어떤 조합을 뽑아도 가방이 고정됐고, 상의 5·하의 3·
-- 신발 3 으로는 점수 상위가 늘 같은 자리로 수렴했다.
--
-- 그래서 실루엣과 색의 대비를 만드는 쪽으로 아홉 벌을 더한다. 특히 가방을 1 → 4 로
-- 늘려 "화이트 티 + 워시드 블랙 데님 + 작은 숄더백 + 앵클부츠" 같은 조합이
-- 실제로 만들어질 수 있게 한다.
--
-- 데모 옷장에만 넣는다. 사용자가 실제로 갖고 있지 않은 옷을 추천하게 만들지 않는다.
-- id 를 고정해 두어 재실행에 안전하다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, material_primary, sub_category, texture_keywords)
VALUES
  -- ─── 가방: 하나뿐이라 늘 고정되던 자리 ───
  -- 큰 토트 하나만 있으면 실루엣이 매번 같아진다. 작고 몸에 붙는 것부터
  -- 늘어지는 호보까지 형태를 갈라 둔다.
  ('0ffd0800-0000-0000-0000-000000000000', '블랙 레더 미니 숄더백', '가방', 'female', 'model_off_duty',
   'black', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 2, 'leather', '숄더백',
   'leather, small structured shoulder bag'),
  ('0ffd0800-0000-0000-0000-000000000001', '다크브라운 스웨이드 호보백', '가방', 'female', 'model_off_duty',
   'dark_brown', 'medium', '어두움', '낮음', '베이직', '중간', '포인트', 1, 'suede', '호보백',
   'suede, slouchy hobo, soft body'),
  ('0ffd0800-0000-0000-0000-000000000002', '블랙 나일론 크로스백', '가방', 'female', 'model_off_duty',
   'black', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 1, 'nylon', '크로스백',
   'nylon, compact crossbody'),

  -- ─── 신발: 스니커즈로 몰리지 않게 ───
  ('0ffd0800-0000-0000-0000-000000000003', '블랙 레더 첼시부츠', '신발', 'female', 'model_off_duty',
   'black', 'medium', '어두움', '낮음', '베이직', '중간', '구조템', 2, 'leather', '첼시부츠',
   'leather, slim chelsea boot, elastic gore'),
  ('0ffd0800-0000-0000-0000-000000000004', '다크그레이 스웨이드 스니커즈', '신발', 'female', 'model_off_duty',
   'dark_gray', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 1, 'suede', '스니커즈',
   'suede, low profile sneaker'),

  -- ─── 상의: 밝은 색으로만 몰려 있어 어두운 베이스가 없었다 ───
  ('0ffd0800-0000-0000-0000-000000000005', '블랙 코튼 크루넥 티셔츠', '상의', 'female', 'model_off_duty',
   'black', 'thin', '어두움', '낮음', '베이직', '가벼움', '베이스', 1, 'cotton', '반팔티셔츠',
   'cotton jersey, plain crewneck'),
  ('0ffd0800-0000-0000-0000-000000000006', '차콜 코튼 후드 스웨트셔츠', '상의', 'female', 'model_off_duty',
   'charcoal', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 1, 'cotton', '후드티',
   'french terry, relaxed hoodie'),

  -- ─── 하의: 스트레이트·슬림뿐이라 실루엣 폭이 좁았다 ───
  ('0ffd0800-0000-0000-0000-000000000007', '인디고 와이드 데님', '하의', 'female', 'model_off_duty',
   'indigo', 'medium', '어두움', '중간', '베이직', '중간', '베이스', 1, 'denim', '데님팬츠',
   'denim, wide leg, rigid'),
  ('0ffd0800-0000-0000-0000-000000000008', '블랙 코튼 스트레이트 치노', '하의', 'female', 'model_off_duty',
   'black', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 2, 'cotton', '치노팬츠',
   'cotton twill, straight leg')
ON DUPLICATE KEY UPDATE id = clothing.id;

-- 계절은 넣지 않는다. 여성 아이템은 `20260910000009` 가 규칙으로 일괄 부여하므로,
-- 여기서 직접 넣으면 그 규칙과 갈린다. 아래 한 줄이 같은 규칙을 이 아홉 벌에도
-- 적용한다 (INSERT IGNORE 라 기존 행에는 영향이 없다).
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, s.season
FROM clothing c
CROSS JOIN (
  SELECT '봄' AS season UNION ALL SELECT '여름' UNION ALL SELECT '가을' UNION ALL SELECT '겨울'
) s
WHERE c.id LIKE '0ffd0800-%'
  AND FIND_IN_SET(
    s.season,
    CASE
      WHEN c.category = '가방' THEN '봄,여름,가을,겨울'
      WHEN c.sub_category IN ('스니커즈', '러닝화') THEN '봄,여름,가을,겨울'
      WHEN c.sub_category IN ('첼시부츠', '웨스턴부츠', '앵클부츠') THEN '가을,겨울'
      WHEN c.sub_category IN ('반팔티셔츠') THEN '봄,여름'
      WHEN c.sub_category IN ('후드티') THEN '봄,가을,겨울'
      ELSE '봄,가을'
    END
  ) > 0;
