-- 여성 엣지 유색 하의 4벌 데모 시드. 개발·데모 DB(default 옷장) 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_bottom_colour.sql
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_bottom_colour_rollback.sql   # 되돌리기
--
-- 왜: 2026-11-18 엣지 홈 추천의 하의가 차콜·워시드 블랙·블랙(사용자 눈에 같은 어두운 색)의 긴
-- 바지로 반복됐다. 엣지 하의 8벌 중 6벌이 어두운 무채색·인디고·그레이의 데님·울·폴리 바지이고, 색이
-- 있는 하의는 올리브 미니스커트 하나다(추운 날에는 다리 판정으로 거의 못 쓴다). 눈에 보이는 소재도
-- 데님·울·폴리·가죽(스커트)·코튼뿐이라 코듀로이·스웨이드가 없고, 추운 날 입을 수 있는 색 하의가
-- 없다. 하의 네 축(색 묶음·종류·기장·소재, `look_signature::BottomLook`) 중 색과 소재를 채운다.
--
-- 고른 기준: 엣지의 형태·소재 대비(가죽·결이 보이는 코듀로이·스웨이드, 비대칭 랩·배럴 레그)이면서
-- 블랙·차콜 상의, 기존 외투(차콜 롱 울 코트·블랙 레더 시어링·페트롤틸 퍼퍼 등)와 함께 실제로 입을
-- 만한 깊은 색. 형광·파스텔은 넣지 않는다. 두 벌은 긴 바지라 추운 날에도 다리 판정이 통과하고,
-- 미디 스커트는 부츠와 함께일 때만 영하에서 통과한다(`serving_ranker::lower_body_fit`). 플럼 벨벳 미니는 온화한 날의 짧은 선택지이며
-- 추울 때는 다른 미니스커트와 같은 다리 보온 판정을 받는다.
-- 가상 샘플이다(특정 상품을 옮기지 않았다).
--
-- 기존 옷의 이름·속성은 바꾸지 않는다. 범위는 아래 네 id 중 user_id = 'default' 인 행뿐이다.
-- 장르 태그는 edgy 하나만 단다 — 다른 무드 옷장에는 들어가지 않는다.
-- 재실행 안전: 한 트랜잭션, 같은 id 가 default 소유로 있으면 시드 값으로 맞추고 계절·태그는 건너뛴다.

START TRANSACTION;

DROP TEMPORARY TABLE IF EXISTS edgy_bottom_colour_ids;
CREATE TEMPORARY TABLE edgy_bottom_colour_ids AS SELECT id FROM clothing WHERE 1 = 0;
INSERT INTO edgy_bottom_colour_ids (id) VALUES
  ('0ffd1200-0000-0000-0000-000000000301'),
  ('0ffd1200-0000-0000-0000-000000000302'),
  ('0ffd1200-0000-0000-0000-000000000303'),
  ('0ffd1200-0000-0000-0000-000000000304');

INSERT INTO clothing
  (id, user_id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, color_temperature, formality_level, silhouette_volume, material_primary,
   sub_category, texture_keywords)
VALUES
  ('0ffd1200-0000-0000-0000-000000000301', 'default', '버건디 레더 스트레이트 팬츠', '하의', 'female', 'edgy',
   '버건디', 'medium', '어두움', '중간', '베이직', '중간', '구조템', 'warm', 3, 'regular', 'leather', '팬츠',
   'visible face: matte burgundy lambskin leather, soft sheen; high waist, straight leg, seam down the front of each leg, full length to the ankle'),
  ('0ffd1200-0000-0000-0000-000000000302', 'default', '보틀그린 코듀로이 배럴 팬츠', '하의', 'female', 'edgy',
   '보틀그린', 'thick', '어두움', '중간', '베이직', '중간', '구조템', 'cool', 2, 'relaxed', 'corduroy', '와이드팬츠',
   'visible face: wide-wale cotton corduroy with clear ridges; curved barrel leg tapering to the hem, high waist, front pleats, full length to the ankle'),
  ('0ffd1200-0000-0000-0000-000000000303', 'default', '러스트 스웨이드 비대칭 랩 미디 스커트', '하의', 'female', 'edgy',
   '러스트', 'medium', '중간', '중간', '베이직', '중간', '약한포인트', 'warm', 3, 'slim', 'suede', '스커트',
   'visible face: brushed rust suede with a soft nap; wrap front with an asymmetric diagonal hem, side buckle, midi length to mid-calf'),
  ('0ffd1200-0000-0000-0000-000000000304', 'default', '딥 플럼 벨벳 슬릿 미니스커트', '하의', 'female', 'edgy',
   '플럼', 'medium', '어두움', '중간', '베이직', '중간', '약한포인트', 'cool', 3, 'regular', 'velvet', '스커트',
   'visible face: deep plum velvet with a directional pile; clean A-line mini skirt, mid-thigh hem, small offset front slit')
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
SELECT c.id, s.season
  FROM edgy_bottom_colour_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default'
  JOIN (SELECT '가을' AS season UNION ALL SELECT '겨울' UNION ALL SELECT '봄') s
 WHERE NOT (c.id = '0ffd1200-0000-0000-0000-000000000302' AND s.season = '봄');

INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT c.id, 'edgy', 'seed'
  FROM edgy_bottom_colour_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default';

DROP TEMPORARY TABLE edgy_bottom_colour_ids;

COMMIT;
