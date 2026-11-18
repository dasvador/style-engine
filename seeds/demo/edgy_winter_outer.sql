-- 여성 엣지 겨울 외투 2벌 데모 시드. 개발·데모 DB(default 옷장) 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_winter_outer.sql
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_winter_outer_rollback.sql   # 되돌리기
--
-- 왜: 2026-11-18 엣지 홈 추천의 추운 날 카드가 차콜 울 롱코트로 반복됐다. 엣지 겨울 외투 5벌
-- (`serving_ranker::is_winter_outer`) 중 셋이 울 코트(차콜 미드카프·코발트 무릎·플럼 발목)이고
-- 나머지 둘은 가죽·스웨이드 겉감의 힙 기장 시어링 안감 재킷이다. 눈에 보이는 겉감이 나일론이거나
-- 시어링 털인 외투, 코쿤·박시처럼 몸에서 떨어지는 실루엣이 없었다. 색·겉감 질감·실루엣·기장
-- 네 축 중 겉감과 실루엣이 비어 있어 두 벌을 더한다.
--
-- 겉감 ≠ 보온재: `texture_keywords` 는 눈에 보이는 겉감(shell/face)과 보온(fill/lining)을 따로
-- 적는다. 다운 충전재를 "다운 소재" 로, 시어링 털 겉면을 "안감" 으로 적지 않는다.
--
-- 근거(사용자가 웹에서 확인해 준 실제 제품 → 아래는 가상 샘플 해석이다):
--   1) COS Funnel-Neck Nylon Down Puffer Jacket
--      https://www.cos.com/en-in/product/funnel-neck-nylon-down-puffer-jacket-1336464001
--      나일론 겉감 + 다운·페더 충전, 밑단 드로코드로 볼륨, 길이 69.6cm. 원 제품은 네이비다.
--      → 페트롤 틸 색의 힙 기장 조형적 코쿤 퍼퍼(색은 가상).
--   2) Coach Shearling Bomber Jacket
--      https://world.coach.com/products/shearling-bomber-jacket/88370%20PKN%20%200.html
--      양가죽 시어링(털이 겉면), 안쪽은 나파 가죽, 리브 밑단, 길이 22 3/4인치(약 58cm).
--      → 밝게 염색한 머스타드 시어링의 박시 봄버(색은 가상).
--
-- 기존 옷의 이름·속성은 바꾸지 않는다. 범위는 아래 두 id 중 user_id = 'default' 인 행뿐이다.
-- 재실행 안전: 한 트랜잭션, 같은 id 가 default 소유로 있으면 시드 값으로 맞추고 계절·태그는 건너뛴다.

START TRANSACTION;

DROP TEMPORARY TABLE IF EXISTS edgy_winter_outer_ids;
CREATE TEMPORARY TABLE edgy_winter_outer_ids AS SELECT id FROM clothing WHERE 1 = 0;
INSERT INTO edgy_winter_outer_ids (id) VALUES
  ('0ffd1200-0000-0000-0000-000000000201'),
  ('0ffd1200-0000-0000-0000-000000000202');

INSERT INTO clothing
  (id, user_id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, color_temperature, formality_level, silhouette_volume, material_primary,
   sub_category, texture_keywords)
VALUES
  ('0ffd1200-0000-0000-0000-000000000201', 'default', '페트롤틸 퍼널넥 코쿤 다운 퍼퍼', '아우터', 'female', 'edgy',
   '페트롤틸', 'thick', '중간', '중간', '베이직', '중간', '포인트', 'cool', 1, 'oversized', 'nylon', '패딩',
   'visible shell: matte nylon, wide channel quilting; insulation: down fill (down and feather); high funnel neck, sculpted cocoon volume gathered by drawcord hem, hip-length'),
  ('0ffd1200-0000-0000-0000-000000000202', 'default', '머스타드 시어링 박시 봄버', '아우터', 'female', 'edgy',
   '머스타드', 'thick', '중간', '높음', '베이직', '무거움', '포인트', 'warm', 2, 'relaxed', 'shearling', '봄버',
   'visible face: dyed curly shearling pile; inside: nappa leather face of the same sheepskin; boxy cropped bomber cut, ribbed hem and cuffs, high-hip length')
AS new
ON DUPLICATE KEY UPDATE
  name = IF(clothing.user_id = 'default', new.name, clothing.name),
  category = IF(clothing.user_id = 'default', new.category, clothing.category),
  gender = IF(clothing.user_id = 'default', new.gender, clothing.gender),
  style_mood = IF(clothing.user_id = 'default', new.style_mood, clothing.style_mood),
  color = IF(clothing.user_id = 'default', new.color, clothing.color),
  thickness = IF(clothing.user_id = 'default', new.thickness, clothing.thickness),
  tone = IF(clothing.user_id = 'default', new.tone, clothing.tone),
  saturation = IF(clothing.user_id = 'default', new.saturation, clothing.saturation),
  style = IF(clothing.user_id = 'default', new.style, clothing.style),
  weight = IF(clothing.user_id = 'default', new.weight, clothing.weight),
  role = IF(clothing.user_id = 'default', new.role, clothing.role),
  color_temperature = IF(clothing.user_id = 'default', new.color_temperature, clothing.color_temperature),
  formality_level = IF(clothing.user_id = 'default', new.formality_level, clothing.formality_level),
  silhouette_volume = IF(clothing.user_id = 'default', new.silhouette_volume, clothing.silhouette_volume),
  material_primary = IF(clothing.user_id = 'default', new.material_primary, clothing.material_primary),
  sub_category = IF(clothing.user_id = 'default', new.sub_category, clothing.sub_category),
  texture_keywords = IF(clothing.user_id = 'default', new.texture_keywords, clothing.texture_keywords);

INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, '겨울'
  FROM edgy_winter_outer_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default';

INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT c.id, 'edgy', 'seed'
  FROM edgy_winter_outer_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default';

DROP TEMPORARY TABLE edgy_winter_outer_ids;

COMMIT;
