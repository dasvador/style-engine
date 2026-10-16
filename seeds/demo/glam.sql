-- 여성 '글램' 무드 데모 시드. 개발·데모 DB 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/glam.sql            # 넣기
--   python3 tools/demo-seed/apply.py seeds/demo/glam_rollback.sql   # 되돌리기
--
-- 왜 마이그레이션이 아닌가: `clothing` 에는 데모 옷과 사용자가 올린 옷을 가르는 컬럼이
-- 없다(사용자 업로드는 무작위 id 에 image_url 이 있다는 관례뿐이다). 마이그레이션은
-- 서버가 뜰 때 운영 DB 에도 자동으로 들어가므로, 구분할 수 없는 구조에서 옷을 일괄로
-- 넣으면 사용자 옷장에 데모 옷이 섞인다. 그래서 무드 칩(`style_mood` 행)도 여기서 같이
-- 넣는다 — 시드를 넣지 않은 DB 에는 후보가 없는 '글램' 칩이 뜨지 않는다.
--
-- 재실행 안전: 새 옷은 고정 id(`0ffd0b00-…`)로 넣고 같은 id 가 있으면 값을 시드대로
-- 맞춘다. 태그·계절·텍스처는 INSERT IGNORE 다. 기존 옷의 행은 고치지 않고, 글램 태그
-- 행만 더한다(`source = 'seed'` — 되돌리기가 이 값으로 정확히 지운다).
--
-- 글램의 정의: 빛을 받는 소재(새틴·메탈릭·벨벳)나 조형적인 실루엣(원숄더·드레이프·
-- 비대칭) 한두 가지가 코디의 중심이고, 나머지는 담백한 기본 아이템이 받친다. 그래서
--   - 포인트는 상의·아우터·가방 중 한 곳에 두고(`role = '포인트'`), 하의와 신발은
--     받쳐 주는 역할(`베이스`·`구조템`·`연결템`)로 둔다. 엔진의 기존 규칙(포인트 2개
--     이상 감점, 하의 포인트 감점, 가방까지 포인트면 감점)이 강한 요소의 충돌을 막는다.
--   - 기본 아이템(블랙 슬랙스·워시드 데님·레더 라이더 등)은 기존 무드에서 빼앗지 않고
--     태그를 하나 더 붙인다(`clothing_style_genre` 는 한 옷에 여러 장르를 허용한다).
--
-- 속성은 실제 옷 그대로 적는다. 얇은 새틴은 thin, 벨벳·메탈릭 니트는 가을·겨울이다.
-- 날씨 게이트를 통과시키려고 값을 바꾸지 않는다. 겨울에는 글램 코트 다섯 벌을 둔다.
--
-- 원피스는 넣지 않는다. 옷장 관례상 원피스는 `하의` 이고, 후보 생성은 상의와 하의를
-- 반드시 함께 고르므로(`recommendation::build_outfit_candidate`) 슬립 드레스 위에 상의가
-- 한 장 더 얹힌 조합이 나온다.

-- ─── 무드 칩 ───
INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('female', 'glam', '글램', '빛나는 소재와 선명한 실루엣으로 존재감을 주는 스타일', 9)
AS new
ON DUPLICATE KEY UPDATE
  mood_label = new.mood_label,
  description = new.description,
  sort_order = new.sort_order;

-- ─── 새 옷 18벌 ───
INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, silhouette_volume, material_primary, sub_category,
   texture_keywords)
VALUES
  -- 상의 — 포인트는 소재나 실루엣 하나다.
  ('0ffd0b00-0000-0000-0000-000000000001', '블랙 원숄더 저지 탑', '상의', 'female', 'glam',
   '블랙', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 3, 'slim', 'jersey',
   '원숄더탑', 'matte stretch jersey, one-shoulder neckline, fitted'),
  ('0ffd0b00-0000-0000-0000-000000000002', '에메랄드 새틴 드레이프 블라우스', '상의', 'female', 'glam',
   '에메랄드', 'thin', '중간', '높음', '베이직', '가벼움', '포인트', 3, 'regular', 'satin',
   '블라우스', 'fluid satin, draped cowl neckline, soft sheen'),
  ('0ffd0b00-0000-0000-0000-000000000003', '샴페인 메탈릭 니트 탑', '상의', 'female', 'glam',
   '샴페인', 'medium', '밝음', '낮음', '베이직', '중간', '포인트', 3, 'regular', 'knit',
   '니트', 'fine lurex knit, subtle metallic sheen, crew neck, long sleeves'),
  ('0ffd0b00-0000-0000-0000-000000000004', '버건디 벨벳 비대칭 넥 탑', '상의', 'female', 'glam',
   '버건디', 'medium', '어두움', '중간', '베이직', '중간', '포인트', 3, 'slim', 'velvet',
   '긴팔탑', 'stretch velvet, asymmetric neckline, long sleeves, fitted'),

  -- 하의 — 받쳐 주는 쪽. 미니스커트는 롱부츠와 함께 가을에 입는다.
  ('0ffd0b00-0000-0000-0000-000000000005', '블랙 드레이프 랩 미니스커트', '하의', 'female', 'glam',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 3, 'slim', 'polyester',
   '스커트', 'matte crepe, draped wrap front, mini length'),
  ('0ffd0b00-0000-0000-0000-000000000006', '샴페인 새틴 슬립 미디스커트', '하의', 'female', 'glam',
   '샴페인', 'thin', '밝음', '낮음', '베이직', '가벼움', '베이스', 3, 'regular', 'satin',
   '스커트', 'bias-cut satin, slip skirt, midi length, soft sheen'),

  -- 아우터
  ('0ffd0b00-0000-0000-0000-000000000007', '블랙 벨벳 테일러드 블레이저', '아우터', 'female', 'glam',
   '블랙', 'medium', '어두움', '낮음', '포멀', '중간', '포인트', 4, 'regular', 'velvet',
   '블레이저', 'cotton velvet, soft pile sheen, single-breasted, peak lapels'),
  ('0ffd0b00-0000-0000-0000-000000000008', '블랙 새틴 봄버 자켓', '아우터', 'female', 'glam',
   '블랙', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 2, 'regular', 'satin',
   '봄버자켓', 'lustrous satin shell, ribbed collar and cuffs, cropped at the waist'),

  -- 신발
  ('0ffd0b00-0000-0000-0000-000000000009', '블랙 레더 니하이 롱부츠', '신발', 'female', 'glam',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '구조템', 3, NULL, 'leather',
   '롱부츠', 'smooth leather, knee-high slim shaft, block heel'),
  ('0ffd0b00-0000-0000-0000-00000000000a', '실버 메탈릭 스트랩 샌들', '신발', 'female', 'glam',
   '실버', 'thin', '밝음', '낮음', '베이직', '가벼움', '연결템', 3, NULL, 'leather',
   '샌들', 'metallic leather, thin straps, low block heel'),
  ('0ffd0b00-0000-0000-0000-00000000000b', '블랙 새틴 포인티드 펌프스', '신발', 'female', 'glam',
   '블랙', 'medium', '어두움', '낮음', '포멀', '중간', '베이스', 4, NULL, 'satin',
   '힐', 'satin upper, pointed toe, mid heel'),

  -- 가방
  ('0ffd0b00-0000-0000-0000-00000000000c', '실버 메탈릭 미니 숄더백', '가방', 'female', 'glam',
   '실버', 'medium', '밝음', '낮음', '베이직', '중간', '포인트', 3, NULL, 'leather',
   '숄더백', 'metallic leather, mini, short shoulder strap'),
  ('0ffd0b00-0000-0000-0000-00000000000d', '블랙 새틴 미니 클러치', '가방', 'female', 'glam',
   '블랙', 'thin', '어두움', '낮음', '포멀', '가벼움', '약한포인트', 4, NULL, 'satin',
   '클러치', 'satin, soft gathered frame, mini clutch'),

  -- 겨울 글램 코트 다섯 벌(2026-10-17 사용자 지정). 단정한 무광 코트는 넣지 않는다 —
  -- 코트도 소재(퍼·시어링)·실루엣(아워글라스)·색(딥 버건디) 중 하나로 글램이어야 한다.
  -- 길이는 세부 종류(롱코트·하프코트·크롭자켓·시어링자켓)에 적어 보온 판정에 쓴다
  -- (`serving_ranker::layer_warmth`). 두께는 안감까지 본 값이다.
  ('0ffd0b00-0000-0000-0000-00000000000e', '에스프레소 브라운 페이크퍼 하프코트', '아우터', 'female', 'glam',
   '에스프레소브라운', 'thick', '어두움', '중간', '베이직', '무거움', '포인트', 3, 'relaxed', 'faux fur',
   '하프코트', 'plush faux fur, hip-length half coat, full but not rounded, wide collar, concealed closure, insulated lining'),
  ('0ffd0b00-0000-0000-0000-00000000000f', '블랙 아워글라스 롱 울 코트', '아우터', 'female', 'glam',
   '블랙', 'thick', '어두움', '낮음', '포멀', '중간', '구조템', 4, 'regular', 'wool',
   '롱코트', 'heavy wool blend, hourglass silhouette, defined shoulders, nipped waist, flared hem, mid-calf length, wide peaked lapels, matte buttons, insulated lining'),
  ('0ffd0b00-0000-0000-0000-000000000010', '아이보리 페이크퍼 크롭 재킷', '아우터', 'female', 'glam',
   '아이보리', 'thick', '밝음', '낮음', '베이직', '중간', '포인트', 3, 'regular', 'faux fur',
   '크롭자켓', 'dense short-pile faux fur, cropped at the waist, round neck, concealed closure, moderately full sleeves, insulated lining'),
  ('0ffd0b00-0000-0000-0000-000000000011', '딥 버건디 벨티드 롱 울 코트', '아우터', 'female', 'glam',
   '버건디', 'thick', '어두움', '높음', '포멀', '중간', '포인트', 4, 'relaxed', 'wool',
   '롱코트', 'heavy matte wool, wide shawl collar, self-fabric belt, below-calf length, relaxed below the waist, insulated lining'),
  ('0ffd0b00-0000-0000-0000-000000000012', '블랙 레더 시어링 재킷', '아우터', 'female', 'glam',
   '블랙', 'thick', '어두움', '낮음', '베이직', '무거움', '포인트', 2, 'relaxed', 'leather',
   '시어링자켓', 'matte black leather, thick shearling lining, shearling collar and cuffs, hip-length, minimal hardware')
AS new
ON DUPLICATE KEY UPDATE
  name = new.name, category = new.category, gender = new.gender,
  style_mood = new.style_mood, color = new.color, thickness = new.thickness,
  tone = new.tone, saturation = new.saturation, style = new.style, weight = new.weight,
  role = new.role, formality_level = new.formality_level,
  silhouette_volume = new.silhouette_volume, material_primary = new.material_primary,
  sub_category = new.sub_category, texture_keywords = new.texture_keywords;

-- ─── 계절 ───
INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES
  ('0ffd0b00-0000-0000-0000-000000000001', '봄'), ('0ffd0b00-0000-0000-0000-000000000001', '여름'),
  ('0ffd0b00-0000-0000-0000-000000000001', '가을'),
  ('0ffd0b00-0000-0000-0000-000000000002', '봄'), ('0ffd0b00-0000-0000-0000-000000000002', '여름'),
  ('0ffd0b00-0000-0000-0000-000000000002', '가을'),
  ('0ffd0b00-0000-0000-0000-000000000003', '가을'), ('0ffd0b00-0000-0000-0000-000000000003', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000004', '가을'), ('0ffd0b00-0000-0000-0000-000000000004', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000005', '봄'), ('0ffd0b00-0000-0000-0000-000000000005', '가을'),
  ('0ffd0b00-0000-0000-0000-000000000006', '봄'), ('0ffd0b00-0000-0000-0000-000000000006', '여름'),
  ('0ffd0b00-0000-0000-0000-000000000006', '가을'),
  ('0ffd0b00-0000-0000-0000-000000000007', '가을'), ('0ffd0b00-0000-0000-0000-000000000007', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000008', '봄'), ('0ffd0b00-0000-0000-0000-000000000008', '가을'),
  ('0ffd0b00-0000-0000-0000-000000000009', '가을'), ('0ffd0b00-0000-0000-0000-000000000009', '겨울'),
  ('0ffd0b00-0000-0000-0000-00000000000a', '봄'), ('0ffd0b00-0000-0000-0000-00000000000a', '여름'),
  ('0ffd0b00-0000-0000-0000-00000000000b', '봄'), ('0ffd0b00-0000-0000-0000-00000000000b', '가을'),
  ('0ffd0b00-0000-0000-0000-00000000000b', '겨울'),
  ('0ffd0b00-0000-0000-0000-00000000000c', '봄'), ('0ffd0b00-0000-0000-0000-00000000000c', '여름'),
  ('0ffd0b00-0000-0000-0000-00000000000c', '가을'), ('0ffd0b00-0000-0000-0000-00000000000c', '겨울'),
  ('0ffd0b00-0000-0000-0000-00000000000d', '봄'), ('0ffd0b00-0000-0000-0000-00000000000d', '여름'),
  ('0ffd0b00-0000-0000-0000-00000000000d', '가을'), ('0ffd0b00-0000-0000-0000-00000000000d', '겨울'),
  ('0ffd0b00-0000-0000-0000-00000000000e', '겨울'),
  ('0ffd0b00-0000-0000-0000-00000000000f', '가을'), ('0ffd0b00-0000-0000-0000-00000000000f', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000010', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000011', '가을'), ('0ffd0b00-0000-0000-0000-000000000011', '겨울'),
  ('0ffd0b00-0000-0000-0000-000000000012', '가을'), ('0ffd0b00-0000-0000-0000-000000000012', '겨울');

-- ─── 텍스처 ───
-- 어휘는 Vision 프롬프트의 규칙(`prompts::texture_world_rule`)을 따른다: 새틴과 어깨를
-- 드러내는 탑은 romantic, 테일러링은 tailoring, 가죽은 leather. 메탈릭 니트와 벨벳 탑은
-- 맞는 값이 없어 비워 둔다 — 새 값을 만들면 어느 규칙에도 걸리지 않고 조용히 남는다.
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd0b00-0000-0000-0000-000000000001', 'romantic'),
  ('0ffd0b00-0000-0000-0000-000000000002', 'romantic'),
  ('0ffd0b00-0000-0000-0000-000000000006', 'romantic'),
  ('0ffd0b00-0000-0000-0000-000000000007', 'tailoring'),
  ('0ffd0b00-0000-0000-0000-000000000009', 'leather'),
  ('0ffd0b00-0000-0000-0000-00000000000a', 'leather'),
  ('0ffd0b00-0000-0000-0000-00000000000c', 'leather'),
  ('0ffd0b00-0000-0000-0000-00000000000f', 'tailoring'),
  ('0ffd0b00-0000-0000-0000-000000000011', 'tailoring'),
  ('0ffd0b00-0000-0000-0000-000000000012', 'leather');

-- ─── 장르 태그 ───
-- 새 옷 18벌과, 포인트를 받쳐 줄 기존 옷 17벌. 기존 옷의 원래 태그는 그대로 남는다.
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, 'glam', 'seed' FROM clothing
WHERE id LIKE '0ffd0b00-0000-0000-0000-%'
   OR id IN (
     '8e2f729e-598a-11f1-af43-264d116ef584', -- 아이보리 새틴 셔츠 블라우스
     '8c004e42-598c-11f1-af43-264d116ef584', -- 블랙 미디 새틴 롱스커트
     'd892aeb0-546b-11f1-af43-264d116ef584', -- 블랙 와이드 슬랙스
     '8e2f7e38-598a-11f1-af43-264d116ef584', -- 블랙 스트레이트 울혼방 슬랙스
     'd892c198-546b-11f1-af43-264d116ef584', -- 워시드 다크블루 와이드 데님
     '0ffd0700-0000-0000-0000-000000000008', -- 블랙 레더 라이더 자켓
     'd892ac44-546b-11f1-af43-264d116ef584', -- 블랙 싱글 테일러드 자켓
     '0ffd0700-0000-0000-0000-00000000000c', -- 블랙 레더 앵클부츠
     '8c00513a-598c-11f1-af43-264d116ef584', -- 블랙 슬링백 키튼힐
     '8e2f8144-598a-11f1-af43-264d116ef584', -- 베이지 스퀘어토 스틸레토 힐
     'd892c832-546b-11f1-af43-264d116ef584', -- 블랙 미니 체인 숄더백
     '8c00537e-598c-11f1-af43-264d116ef584', -- 실버 체인 미니 크로스백
     -- 담백한 기본 아이템. 포인트가 있는 아우터·하의 아래에 받친다. 엔진은 아우터 안의
     -- 강한 상의를 감점하고(이너 규칙), 홈의 '일상' 은 평균 격식 2.5 를 넘기면 감점한다 —
     -- 기본 아이템이 없으면 선선한 날의 글램은 전부 이 두 감점을 받는다. 밝은 색도 같이
     -- 둔다. 블랙만으로 채우면 사용자 취향 신호(too_dark)에 계속 걸린다.
     '0ffd0700-0000-0000-0000-000000000000', -- 화이트 코튼 크루넥 티셔츠
     '0ffd0700-0000-0000-0000-000000000003', -- 블랙 리브 탱크탑
     'd8929e84-546b-11f1-af43-264d116ef584', -- 블랙 리브드 슬림 니트
     '8c004cbc-598c-11f1-af43-264d116ef584', -- 화이트 와이드 데님 팬츠
     '8c00504a-598c-11f1-af43-264d116ef584'  -- 누드베이지 스퀘어토 플랫 슈즈
   );

-- 단정한 무광 울 코트는 글램이 아니다(2026-10-17 사용자). 예전 시드가 붙인 태그를 뗀다.
DELETE FROM clothing_style_genre
WHERE clothing_id = '8e2f7d70-598a-11f1-af43-264d116ef584' -- 크림 울혼방 롱 코트
  AND style_genre = 'glam' AND source = 'seed';
