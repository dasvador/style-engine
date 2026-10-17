-- 여성 스트리트·스포티 캐주얼의 컬러 상의·아우터 데모 시드. 개발·데모 DB 전용이다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/street_sporty_color.sql
--   python3 tools/demo-seed/apply.py seeds/demo/street_sporty_color_rollback.sql   # 되돌리기
--
-- 왜: 2026-10-17 실제 홈 추천 점검에서 두 무드의 최종 카드는 상의·아우터가 전부 기본색이었다
-- (스트리트 0/7, 스포티 0/9). 옷장의 상의·아우터 11벌·9벌에 기본색이 아닌 옷이 한 벌도
-- 없었다. 같은 옷을 색만 바꿔 넣지 않고, 무드마다 상의 3벌·아우터 3벌을 얇은 날·보통·추운
-- 날에 나눠 둔다. 색 있는 코디와 기본색 코디가 둘 다 나올 수 있도록 기존 옷은 그대로 둔다.
--
-- 속성은 실제 옷 그대로다. 그래픽 티·트레이닝 탑처럼 색이 시선을 끄는 옷은 약한포인트,
-- 하늘색 드라이핏 티·세이지 롱슬리브처럼 기본 상의는 베이스다. 보온 안감은 다운을 채운
-- 패딩 두 벌에만 적는다(`serving_ranker::has_confirmed_insulation`). 스포티 캐주얼은 일상복이라
-- 전문 운동복(브라탑·레깅스)은 넣지 않는다.
--
-- 재실행 안전: 고정 id(`0ffd0e00-…`). 같은 id 가 있으면 값을 시드대로 맞추고, 계절·텍스처·
-- 태그는 INSERT IGNORE 다. 태그는 해당 무드 하나만 붙인다. 기존 옷·태그·무드 칩은 건드리지
-- 않는다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, silhouette_volume, material_primary, sub_category,
   texture_keywords)
VALUES
  -- 스트리트 — 레드·블루·라벤더 상의, 그린·러스트·버건디 아우터
  ('0ffd0e00-0000-0000-0000-000000000001', '워시드 체리레드 박시 그래픽 티셔츠', '상의', 'female', 'street',
   '체리레드', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 1, 'relaxed', 'cotton',
   '티셔츠', 'heavy washed cotton jersey, boxy fit, faded graphic print, crew neck, short sleeves'),
  ('0ffd0e00-0000-0000-0000-000000000002', '코발트 크롭 후드 스웨트셔츠', '상의', 'female', 'street',
   '코발트', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 1, 'relaxed', 'cotton',
   '후드티', 'brushed french terry, cropped pullover hoodie, kangaroo pocket'),
  ('0ffd0e00-0000-0000-0000-000000000003', '라벤더 박시 코튼 롱슬리브', '상의', 'female', 'street',
   '라벤더', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 1, 'oversized', 'cotton',
   '긴팔티셔츠', 'cotton jersey, boxy oversized fit, dropped shoulders, long sleeves'),
  ('0ffd0e00-0000-0000-0000-000000000004', '포레스트그린 나일론 봄버', '아우터', 'female', 'street',
   '포레스트그린', 'medium', '어두움', '중간', '베이직', '중간', '약한포인트', 2, 'regular', 'nylon',
   '봄버자켓', 'nylon shell, ribbed collar and cuffs, zip front, thin lining'),
  ('0ffd0e00-0000-0000-0000-000000000005', '러스트 나일론 코치 자켓', '아우터', 'female', 'street',
   '러스트', 'thin', '중간', '중간', '베이직', '가벼움', '연결템', 1, 'relaxed', 'nylon',
   '코치자켓', 'nylon coach jacket, snap front, drawcord hem, unlined'),
  ('0ffd0e00-0000-0000-0000-000000000006', '버건디 퀼팅 숏 패딩', '아우터', 'female', 'street',
   '버건디', 'thick', '어두움', '중간', '베이직', '무거움', '약한포인트', 1, 'relaxed', 'nylon',
   '패딩', 'quilted matte nylon shell, down fill, hip-length, stand collar'),

  -- 스포티 캐주얼 — 라즈베리·스카이블루·세이지 상의, 코발트·틸·머스타드 아우터
  ('0ffd0e00-0000-0000-0000-000000000007', '라즈베리 래글런 트레이닝 탑', '상의', 'female', 'sporty_casual',
   '라즈베리', 'medium', '중간', '높음', '스포츠', '중간', '약한포인트', 1, 'regular', 'polyester',
   '긴팔티셔츠', 'brushed polyester, raglan sleeves, crew neck, relaxed fit'),
  ('0ffd0e00-0000-0000-0000-000000000008', '스카이블루 드라이핏 반팔티', '상의', 'female', 'sporty_casual',
   '스카이블루', 'thin', '밝음', '낮음', '스포츠', '가벼움', '베이스', 1, 'relaxed', 'polyester',
   '반팔티셔츠', 'dry-fit, moisture wicking, relaxed fit, short sleeves'),
  ('0ffd0e00-0000-0000-0000-000000000009', '세이지그린 기능성 롱슬리브', '상의', 'female', 'sporty_casual',
   '세이지그린', 'thin', '중간', '낮음', '스포츠', '가벼움', '베이스', 1, 'slim', 'polyester',
   '긴팔티셔츠', 'lightweight stretch jersey, quick-dry, thumbholes, long sleeves'),
  ('0ffd0e00-0000-0000-0000-00000000000a', '코발트 배색 트랙재킷', '아우터', 'female', 'sporty_casual',
   '코발트', 'thin', '중간', '높음', '스포츠', '가벼움', '약한포인트', 1, 'regular', 'polyester',
   '트랙재킷', 'tricot track jacket, off-white side stripes, zip front, stand collar'),
  ('0ffd0e00-0000-0000-0000-00000000000b', '틸 경량 윈드브레이커', '아우터', 'female', 'sporty_casual',
   '틸', 'thin', '중간', '중간', '스포츠', '가벼움', '연결템', 1, 'relaxed', 'nylon',
   '윈드브레이커', 'packable ripstop nylon, hooded, mesh half lining'),
  ('0ffd0e00-0000-0000-0000-00000000000c', '머스타드 경량 다운 재킷', '아우터', 'female', 'sporty_casual',
   '머스타드', 'medium', '중간', '중간', '스포츠', '중간', '약한포인트', 1, 'regular', 'nylon',
   '패딩', 'lightweight quilted shell, down fill, packable, hip-length')
AS new
ON DUPLICATE KEY UPDATE
  name = new.name, category = new.category, gender = new.gender,
  style_mood = new.style_mood, color = new.color, thickness = new.thickness,
  tone = new.tone, saturation = new.saturation, style = new.style, weight = new.weight,
  role = new.role, formality_level = new.formality_level,
  silhouette_volume = new.silhouette_volume, material_primary = new.material_primary,
  sub_category = new.sub_category, texture_keywords = new.texture_keywords;

INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES
  ('0ffd0e00-0000-0000-0000-000000000001', '봄'), ('0ffd0e00-0000-0000-0000-000000000001', '여름'),
  ('0ffd0e00-0000-0000-0000-000000000001', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000002', '봄'), ('0ffd0e00-0000-0000-0000-000000000002', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000002', '겨울'),
  ('0ffd0e00-0000-0000-0000-000000000003', '봄'), ('0ffd0e00-0000-0000-0000-000000000003', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000004', '봄'), ('0ffd0e00-0000-0000-0000-000000000004', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000005', '봄'), ('0ffd0e00-0000-0000-0000-000000000005', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000006', '겨울'),
  ('0ffd0e00-0000-0000-0000-000000000007', '봄'), ('0ffd0e00-0000-0000-0000-000000000007', '가을'),
  ('0ffd0e00-0000-0000-0000-000000000008', '봄'), ('0ffd0e00-0000-0000-0000-000000000008', '여름'),
  ('0ffd0e00-0000-0000-0000-000000000009', '봄'), ('0ffd0e00-0000-0000-0000-000000000009', '여름'),
  ('0ffd0e00-0000-0000-0000-000000000009', '가을'),
  ('0ffd0e00-0000-0000-0000-00000000000a', '봄'), ('0ffd0e00-0000-0000-0000-00000000000a', '가을'),
  ('0ffd0e00-0000-0000-0000-00000000000b', '봄'), ('0ffd0e00-0000-0000-0000-00000000000b', '여름'),
  ('0ffd0e00-0000-0000-0000-00000000000b', '가을'),
  ('0ffd0e00-0000-0000-0000-00000000000c', '가을'), ('0ffd0e00-0000-0000-0000-00000000000c', '겨울');

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd0e00-0000-0000-0000-000000000002', 'sweat'),
  ('0ffd0e00-0000-0000-0000-000000000004', 'nylon'),
  ('0ffd0e00-0000-0000-0000-000000000005', 'nylon'),
  ('0ffd0e00-0000-0000-0000-000000000006', 'nylon'),
  ('0ffd0e00-0000-0000-0000-00000000000b', 'nylon'),
  ('0ffd0e00-0000-0000-0000-00000000000c', 'nylon');

INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, style_mood, 'seed' FROM clothing WHERE id LIKE '0ffd0e00-0000-0000-0000-%';
