-- 엣지 무드의 컬러 상의·아우터 데모 시드. `edgy.sql` 다음에 넣는다 — 개발·데모 DB 전용.
--
--   python3 tools/demo-seed/apply.py seeds/demo/edgy.sql
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_color.sql
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_color_rollback.sql   # 이 시드만 되돌리기
--
-- 왜 따로 두는가: `edgy.sql` 의 상의 중 뚜렷한 유채색은 코발트 니트 하나, 아우터는 검정·
-- 차콜·다크브라운뿐이었다. 2026-10-17 실제 추천(16°C·2°C)의 최종 카드 상의·아우터가 전부
-- 무채색이었다. 엣지의 형태(비대칭·각진 어깨·박시 크롭·가죽과 직물의 대비)를 그대로 갖춘
-- 컬러 옷을 더한다. 색만 있는 평범한 옷은 넣지 않는다 — 무드 게이트는 색으로 엣지를
-- 판단하지 않는다(`mood_gate::is_edgy_piece`).
--
-- 속성은 실제 옷 그대로다. 포인트 옷은 포인트로 적고(점수를 위해 베이스로 낮추지 않는다),
-- 가죽 크롭 재킷·코튼 재킷은 안감을 적지 않는다 — 간절기 옷이라 2°C 의 겨울 외투로 통과하지
-- 않는다. 겨울 코트 두 벌만 두꺼운 울과 보온 안감이다.
--
-- 재실행 안전: 고정 id(`0ffd0d00-…`, `edgy.sql` 의 `0ffd0c00-…` 와 겹치지 않는다). 같은 id 가
-- 있으면 값을 시드대로 맞추고, 계절·텍스처·태그는 INSERT IGNORE 다. 무드 칩과 기존 옷은
-- 건드리지 않는다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, silhouette_volume, material_primary, sub_category,
   texture_keywords)
VALUES
  -- 상의 — 레드·바이올렛·틸·옐로그린. 기존 엣지 상의와 같은 형태를 색만 바꿔 넣지 않는다.
  ('0ffd0d00-0000-0000-0000-000000000001', '체리 레드 사선 헴 보트넥 저지 탑', '상의', 'female', 'edgy',
   '체리레드', 'thin', '중간', '높음', '베이직', '가벼움', '약한포인트', 2, 'slim', 'jersey',
   '긴팔탑', 'matte cotton jersey, wide boat neckline, diagonal asymmetric hem, long sleeves, fitted'),
  ('0ffd0d00-0000-0000-0000-000000000002', '딥 바이올렛 스트럭처드 숄더 니트', '상의', 'female', 'edgy',
   '딥바이올렛', 'medium', '어두움', '높음', '베이직', '중간', '포인트', 2, 'slim', 'knit',
   '니트', 'fine wool-blend knit, structured padded shoulders, crew neck, long sleeves, fitted body'),
  ('0ffd0d00-0000-0000-0000-000000000003', '틸 오프센터 여밈 크롭 포플린 셔츠', '상의', 'female', 'edgy',
   '틸', 'thin', '중간', '높음', '베이직', '가벼움', '약한포인트', 3, 'regular', 'cotton',
   '셔츠', 'crisp cotton poplin, asymmetric off-centre placket, sharp pointed collar, boxy cropped length, long sleeves'),
  ('0ffd0d00-0000-0000-0000-000000000004', '옐로그린 비대칭 넥 코튼 탱크', '상의', 'female', 'edgy',
   '옐로그린', 'thin', '밝음', '높음', '베이직', '가벼움', '약한포인트', 2, 'slim', 'cotton',
   '탱크탑', 'ribbed cotton, asymmetric one-sided square neckline, wide straps, fitted'),

  -- 아우터 — 간절기 두 벌(안감 없음), 겨울 코트 두 벌(두꺼운 울 + 보온 안감).
  ('0ffd0d00-0000-0000-0000-000000000005', '버건디 박시 크롭 레더 재킷', '아우터', 'female', 'edgy',
   '버건디', 'medium', '어두움', '중간', '베이직', '중간', '포인트', 3, 'regular', 'leather',
   '크롭자켓', 'matte lambskin leather, boxy cropped cut, sculpted shoulders, concealed snap front, unlined'),
  ('0ffd0d00-0000-0000-0000-000000000006', '페트롤 블루 스트럭처드 크롭 코튼 재킷', '아우터', 'female', 'edgy',
   '페트롤블루', 'medium', '중간', '높음', '베이직', '중간', '포인트', 3, 'regular', 'cotton',
   '크롭자켓', 'heavy cotton twill, structured square shoulders, cropped at the waist, asymmetric zip front, two flap pockets'),
  ('0ffd0d00-0000-0000-0000-000000000007', '코발트 하이넥 비대칭 여밈 울 코트', '아우터', 'female', 'edgy',
   '코발트', 'thick', '중간', '높음', '베이직', '중간', '포인트', 3, 'regular', 'wool',
   '코트', 'heavy wool, high funnel neck, asymmetric wrap front with concealed snaps, knee-length, insulated lining'),
  ('0ffd0d00-0000-0000-0000-000000000008', '플럼 스트럭처드 숄더 롱 울 코트', '아우터', 'female', 'edgy',
   '플럼', 'thick', '어두움', '중간', '베이직', '중간', '포인트', 3, 'regular', 'wool',
   '롱코트', 'heavy wool, sharp structured shoulders, concealed single-breasted placket, clean straight line, ankle-length, insulated lining')
AS new
ON DUPLICATE KEY UPDATE
  name = new.name, category = new.category, gender = new.gender,
  style_mood = new.style_mood, color = new.color, thickness = new.thickness,
  tone = new.tone, saturation = new.saturation, style = new.style, weight = new.weight,
  role = new.role, formality_level = new.formality_level,
  silhouette_volume = new.silhouette_volume, material_primary = new.material_primary,
  sub_category = new.sub_category, texture_keywords = new.texture_keywords;

INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES
  ('0ffd0d00-0000-0000-0000-000000000001', '봄'), ('0ffd0d00-0000-0000-0000-000000000001', '가을'),
  ('0ffd0d00-0000-0000-0000-000000000002', '가을'), ('0ffd0d00-0000-0000-0000-000000000002', '겨울'),
  ('0ffd0d00-0000-0000-0000-000000000003', '봄'), ('0ffd0d00-0000-0000-0000-000000000003', '여름'),
  ('0ffd0d00-0000-0000-0000-000000000003', '가을'),
  ('0ffd0d00-0000-0000-0000-000000000004', '봄'), ('0ffd0d00-0000-0000-0000-000000000004', '여름'),
  ('0ffd0d00-0000-0000-0000-000000000005', '봄'), ('0ffd0d00-0000-0000-0000-000000000005', '가을'),
  ('0ffd0d00-0000-0000-0000-000000000006', '봄'), ('0ffd0d00-0000-0000-0000-000000000006', '가을'),
  ('0ffd0d00-0000-0000-0000-000000000007', '가을'), ('0ffd0d00-0000-0000-0000-000000000007', '겨울'),
  ('0ffd0d00-0000-0000-0000-000000000008', '가을'), ('0ffd0d00-0000-0000-0000-000000000008', '겨울');

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd0d00-0000-0000-0000-000000000005', 'leather'),
  ('0ffd0d00-0000-0000-0000-000000000007', 'tailoring'),
  ('0ffd0d00-0000-0000-0000-000000000008', 'tailoring');

INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, 'edgy', 'seed' FROM clothing WHERE id LIKE '0ffd0d00-0000-0000-0000-%';
