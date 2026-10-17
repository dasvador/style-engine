-- 여성 '팝' 무드 데모 시드. 개발·데모 DB 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/pop.sql
--   python3 tools/demo-seed/apply.py seeds/demo/pop_rollback.sql   # 되돌리기
--
-- 마이그레이션이 아닌 이유는 `glam.sql` 과 같다(데모 옷과 사용자 옷을 가르는 컬럼이 없다). 무드
-- 칩(`style_mood` 행)도 여기서 같이 넣는다 — 시드가 없는 DB 에는 후보 없는 '팝' 칩이 뜨지 않는다.
--
-- 팝: 선명한 색의 조합, 컬러 그래픽·기하 패턴, 배색 아우터로 시선을 끄는 성인 여성의 일상복.
-- 밝은 옷 한 벌만으로는 아니다. 세 방향이 모두 나올 수 있게 둔다:
--   - 색 조합: 코발트·레드·오렌지·그린·핑크 계열의 상의와 하의를 서로 섞는다.
--   - 배색·그래픽 옷: 컬러블록 스웨트셔츠·멀티 스트라이프 셔츠·컬러 그래픽 티·체커보드 니트·
--     스트라이프 팬츠. 흑백 무늬는 넣지 않는다(흑백은 팝의 색 근거가 아니다).
--   - 배색 아우터: 컬러블록 바람막이(간절기), 체크 울 코트·컬러블록 패딩(겨울, 보온 안감·다운).
-- 기본색 옷(화이트 티·데님·슬랙스)과 신발·가방은 기존 옷에 태그만 더해 받친다.
--
-- 속성은 실제 옷 그대로다. 색 옷이라고 모두 포인트로 적지 않고, 보온 안감은 겨울 코트·패딩에만
-- 적는다. 배색의 색은 `color`("코발트/옐로")와 설명에 적는다. 원피스·소품은 넣지 않는다.
--
-- 재실행 안전: 새 옷은 고정 id(`0ffd0f00-…`). 같은 id 가 있으면 값을 시드대로 맞추고, 계절·텍스처·
-- 태그는 INSERT IGNORE 다. 기존 옷의 행·다른 장르 태그는 고치지 않는다(`source = 'seed'` 로 팝
-- 태그만 더한다 — 되돌리기가 이 시드의 옷 목록으로만 지운다).

-- ─── 무드 칩 ───
INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('female', 'pop', '팝', '선명한 색의 조합과 컬러 그래픽·배색으로 유쾌하게 시선을 끄는 스타일', 11)
AS new
ON DUPLICATE KEY UPDATE
  mood_label = new.mood_label,
  description = new.description,
  sort_order = new.sort_order;

-- ─── 새 옷 16벌 ───
INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, silhouette_volume, material_primary, sub_category,
   texture_keywords)
VALUES
  -- 상의
  ('0ffd0f00-0000-0000-0000-000000000001', '코발트 블루 크롭 니트', '상의', 'female', 'pop',
   '코발트', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 2, 'regular', 'knit',
   '니트', 'soft cotton-blend knit, cropped length, crew neck, long sleeves'),
  ('0ffd0f00-0000-0000-0000-000000000002', '레드 핑크 컬러블록 스웨트셔츠', '상의', 'female', 'pop',
   '레드/핑크', 'medium', '중간', '높음', '베이직', '중간', '포인트', 1, 'relaxed', 'cotton',
   '맨투맨', 'brushed cotton fleece, colour-block panels in red and pink, ribbed hem'),
  ('0ffd0f00-0000-0000-0000-000000000003', '멀티 스트라이프 포플린 셔츠', '상의', 'female', 'pop',
   '화이트/레드/블루', 'thin', '밝음', '높음', '베이직', '가벼움', '포인트', 2, 'relaxed', 'cotton',
   '셔츠', 'crisp cotton poplin, bold stripe in red, cobalt blue and white, relaxed fit'),
  ('0ffd0f00-0000-0000-0000-000000000004', '옐로 컬러 그래픽 반팔티', '상의', 'female', 'pop',
   '옐로', 'thin', '밝음', '높음', '베이직', '가벼움', '약한포인트', 1, 'regular', 'cotton',
   '반팔티셔츠', 'cotton jersey, abstract print in cobalt blue and pink on yellow, short sleeves'),
  ('0ffd0f00-0000-0000-0000-000000000005', '라일락 슬리브리스 니트 탑', '상의', 'female', 'pop',
   '라일락', 'thin', '밝음', '중간', '베이직', '가벼움', '베이스', 2, 'slim', 'knit',
   '탱크탑', 'fine rib knit, sleeveless, crew neck, fitted'),
  ('0ffd0f00-0000-0000-0000-000000000006', '그린 화이트 체커보드 니트', '상의', 'female', 'pop',
   '그린/화이트', 'medium', '중간', '높음', '베이직', '중간', '포인트', 2, 'regular', 'knit',
   '니트', 'checkerboard intarsia knit in kelly green and white, crew neck'),

  -- 하의
  ('0ffd0f00-0000-0000-0000-000000000007', '체리레드 A라인 미디 스커트', '하의', 'female', 'pop',
   '체리레드', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 2, 'regular', 'cotton',
   '스커트', 'cotton twill, a-line, midi length'),
  ('0ffd0f00-0000-0000-0000-000000000008', '코발트 와이드 팬츠', '하의', 'female', 'pop',
   '코발트', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 2, 'relaxed', 'cotton',
   '와이드팬츠', 'cotton twill, high waist, wide leg, full length'),
  ('0ffd0f00-0000-0000-0000-000000000009', '탠저린 플리츠 미니스커트', '하의', 'female', 'pop',
   '오렌지', 'medium', '밝음', '높음', '베이직', '중간', '약한포인트', 2, 'regular', 'polyester',
   '스커트', 'crisp pleats, mini length'),
  ('0ffd0f00-0000-0000-0000-00000000000a', '핑크 와이드 스트라이프 팬츠', '하의', 'female', 'pop',
   '핑크/화이트', 'thin', '밝음', '중간', '베이직', '가벼움', '약한포인트', 2, 'relaxed', 'linen',
   '와이드팬츠', 'linen-cotton, wide stripe in pink and white, elastic waist, wide leg'),
  ('0ffd0f00-0000-0000-0000-00000000000b', '켈리그린 코듀로이 스트레이트 팬츠', '하의', 'female', 'pop',
   '켈리그린', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 2, 'regular', 'corduroy',
   '팬츠', 'fine-wale corduroy, straight leg, full length'),

  -- 아우터 — 간절기 두 벌(안감 없음), 니트 가디건, 겨울 두 벌(보온 안감·다운)
  ('0ffd0f00-0000-0000-0000-00000000000c', '코발트 옐로 컬러블록 바람막이', '아우터', 'female', 'pop',
   '코발트/옐로', 'thin', '중간', '높음', '스포츠', '가벼움', '포인트', 1, 'relaxed', 'nylon',
   '윈드브레이커', 'lightweight nylon, colour-block panels in cobalt blue and yellow, zip front, unlined'),
  ('0ffd0f00-0000-0000-0000-00000000000d', '레드 크롭 트러커 재킷', '아우터', 'female', 'pop',
   '레드', 'medium', '중간', '높음', '베이직', '중간', '약한포인트', 2, 'regular', 'cotton',
   '트러커자켓', 'cotton twill, cropped trucker cut, chest pockets'),
  ('0ffd0f00-0000-0000-0000-00000000000e', '라벤더 니트 가디건', '아우터', 'female', 'pop',
   '라벤더', 'medium', '밝음', '중간', '베이직', '중간', '연결템', 2, 'regular', 'knit',
   '가디건', 'chunky cotton knit cardigan, button front'),
  ('0ffd0f00-0000-0000-0000-00000000000f', '그린 레드 체크 울 코트', '아우터', 'female', 'pop',
   '그린/레드', 'thick', '중간', '높음', '베이직', '무거움', '포인트', 3, 'regular', 'wool',
   '코트', 'heavy wool, bold check in green and red, knee-length, insulated lining'),
  ('0ffd0f00-0000-0000-0000-000000000010', '핑크 오렌지 컬러블록 패딩', '아우터', 'female', 'pop',
   '핑크/오렌지', 'thick', '밝음', '높음', '베이직', '무거움', '포인트', 1, 'relaxed', 'nylon',
   '패딩', 'colour-block puffer in pink and orange, down fill, hip-length, stand collar')
AS new
ON DUPLICATE KEY UPDATE
  name = new.name, category = new.category, gender = new.gender,
  style_mood = new.style_mood, color = new.color, thickness = new.thickness,
  tone = new.tone, saturation = new.saturation, style = new.style, weight = new.weight,
  role = new.role, formality_level = new.formality_level,
  silhouette_volume = new.silhouette_volume, material_primary = new.material_primary,
  sub_category = new.sub_category, texture_keywords = new.texture_keywords;

INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES
  ('0ffd0f00-0000-0000-0000-000000000001', '봄'), ('0ffd0f00-0000-0000-0000-000000000001', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000001', '겨울'),
  ('0ffd0f00-0000-0000-0000-000000000002', '봄'), ('0ffd0f00-0000-0000-0000-000000000002', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000002', '겨울'),
  ('0ffd0f00-0000-0000-0000-000000000003', '봄'), ('0ffd0f00-0000-0000-0000-000000000003', '여름'),
  ('0ffd0f00-0000-0000-0000-000000000003', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000004', '봄'), ('0ffd0f00-0000-0000-0000-000000000004', '여름'),
  ('0ffd0f00-0000-0000-0000-000000000005', '봄'), ('0ffd0f00-0000-0000-0000-000000000005', '여름'),
  ('0ffd0f00-0000-0000-0000-000000000006', '봄'), ('0ffd0f00-0000-0000-0000-000000000006', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000006', '겨울'),
  ('0ffd0f00-0000-0000-0000-000000000007', '봄'), ('0ffd0f00-0000-0000-0000-000000000007', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000008', '봄'), ('0ffd0f00-0000-0000-0000-000000000008', '가을'),
  ('0ffd0f00-0000-0000-0000-000000000008', '겨울'),
  ('0ffd0f00-0000-0000-0000-000000000009', '봄'), ('0ffd0f00-0000-0000-0000-000000000009', '여름'),
  ('0ffd0f00-0000-0000-0000-000000000009', '가을'),
  ('0ffd0f00-0000-0000-0000-00000000000a', '여름'),
  ('0ffd0f00-0000-0000-0000-00000000000b', '가을'), ('0ffd0f00-0000-0000-0000-00000000000b', '겨울'),
  ('0ffd0f00-0000-0000-0000-00000000000c', '봄'), ('0ffd0f00-0000-0000-0000-00000000000c', '가을'),
  ('0ffd0f00-0000-0000-0000-00000000000d', '봄'), ('0ffd0f00-0000-0000-0000-00000000000d', '가을'),
  ('0ffd0f00-0000-0000-0000-00000000000e', '봄'), ('0ffd0f00-0000-0000-0000-00000000000e', '가을'),
  ('0ffd0f00-0000-0000-0000-00000000000f', '겨울'),
  ('0ffd0f00-0000-0000-0000-000000000010', '겨울');

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd0f00-0000-0000-0000-000000000002', 'sweat'),
  ('0ffd0f00-0000-0000-0000-00000000000c', 'nylon'),
  ('0ffd0f00-0000-0000-0000-00000000000f', 'tailoring'),
  ('0ffd0f00-0000-0000-0000-000000000010', 'nylon');

-- ─── 장르 태그 ───
-- 새 옷 16벌과, 색 옷을 받쳐 줄 기존 기본 옷·신발·가방 13벌. 기존 옷의 원래 태그는 그대로다.
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, 'pop', 'seed' FROM clothing
WHERE id LIKE '0ffd0f00-0000-0000-0000-%'
   OR id IN (
     '0ffd0700-0000-0000-0000-000000000000', -- 화이트 코튼 크루넥 티셔츠
     'd8929e84-546b-11f1-af43-264d116ef584', -- 블랙 리브드 슬림 니트
     '0ffd0800-0000-0000-0000-000000000007', -- 인디고 와이드 데님
     '8c004cbc-598c-11f1-af43-264d116ef584', -- 화이트 와이드 데님 팬츠
     'd892aeb0-546b-11f1-af43-264d116ef584', -- 블랙 와이드 슬랙스
     '0ffd0700-0000-0000-0000-000000000007', -- 크림 코튼 와이드 팬츠
     '0ffd0700-0000-0000-0000-00000000000a', -- 화이트 레더 로우 스니커즈
     'd892c5d0-546b-11f1-af43-264d116ef584', -- 화이트 캔버스 하이탑 스니커즈
     '0ffd0700-0000-0000-0000-00000000000c', -- 블랙 레더 앵클부츠
     '0ffd0b00-0000-0000-0000-000000000009', -- 블랙 레더 니하이 롱부츠
     '0ffd0c00-0000-0000-0000-000000000010', -- 블랙 레더 청키 피셔맨 샌들
     '0ffd0800-0000-0000-0000-000000000000', -- 블랙 레더 미니 숄더백
     '8c0052ca-598c-11f1-af43-264d116ef584'  -- 화이트 캔버스 토트백
   );
