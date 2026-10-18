-- 여성 '엣지' 무드 데모 시드. 개발·데모 DB 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/edgy.sql            # 넣기
--   python3 tools/demo-seed/apply.py seeds/demo/edgy_rollback.sql   # 되돌리기
--
-- 마이그레이션이 아닌 이유는 `glam.sql` 과 같다: `clothing` 에는 데모 옷과 사용자가 올린
-- 옷을 가르는 컬럼이 없어, 서버가 뜰 때 자동으로 들어가면 운영 사용자 옷장에 데모 옷이
-- 섞인다. 무드 칩(`style_mood` 행)도 여기서 같이 넣는다 — 시드를 넣지 않은 DB 에는 후보가
-- 없는 '엣지' 칩이 뜨지 않는다.
--
-- 재실행 안전: 새 옷은 고정 id(`0ffd0c00-…`)로 넣고 같은 id 가 있으면 값을 시드대로
-- 맞춘다. 계절·텍스처·태그는 INSERT IGNORE 다. 기존 옷은 행을 고치지 않고 엣지 태그 행만
-- 더한다(`source = 'seed'` — 되돌리기가 이 값으로 정확히 지운다). 기존 옷의 원래 무드
-- 태그는 지우지 않는다.
--
-- 엣지의 정의: 비대칭, 구조적인 실루엣(각진 어깨·배럴 레그·박시 크롭), 가죽과 니트·데님·
-- 직물의 대비, 날렵하거나 묵직한 비율로 취향과 존재감이 드러나는 도시적인 스타일이다.
-- 글램이 광택·곡선으로 시선을 끈다면 엣지는 형태와 대비로 끈다. 스트리트(후디·조거·
-- 스니커즈)와 모던 시크(단정한 테일러링)와도 다르다. 라이더 + 검은 스키니 하나로 굳지
-- 않도록 실루엣이 다른 착장이 나올 수 있게 구성한다:
--   - 비대칭 상의 + 구조적인 와이드·배럴 팬츠 + 앵클부츠
--   - 레더 미디 스커트 + 몸에 맞는 니트 + 형태가 있는 아우터
--   - 워시드 데님 + 박시 크롭 재킷이나 비대칭 헴 상의
--   - 추운 날: 보온 안감이 실제로 적힌 구조적인 롱코트·시어링 재킷 + 긴 하의
-- 흑백 외에 코발트·버건디·올리브·브라운을 섞는다.
--
-- 속성은 실제 옷 그대로 적는다. 보온 안감이 없는 레더 블레이저·크롭 재킷은 안감을 적지
-- 않는다 — 그래야 2°C 에 겨울 외투로 통과하지 않는다(`serving_ranker::is_winter_outer`).
-- 원피스는 넣지 않는다(슬롯 구조가 아직 원피스를 지원하지 않는다 — `glam.sql` 참고).

-- ─── 무드 칩 ───
INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('female', 'edgy', '엣지', '비대칭과 구조적인 실루엣, 가죽과 니트·데님의 대비로 존재감을 주는 도시적인 스타일', 10)
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
  -- 상의 — 비대칭 헴·비대칭 넥·각진 어깨·비대칭 여밈. 여름에 입을 얇은 탑도 하나 둔다.
  ('0ffd0c00-0000-0000-0000-000000000001', '차콜 비대칭 헴 저지 롱슬리브', '상의', 'female', 'edgy',
   '차콜', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 2, 'slim', 'jersey',
   '긴팔탑', 'matte cotton jersey, asymmetric hem dipping longer on one side, long sleeves, close fit'),
  ('0ffd0c00-0000-0000-0000-000000000002', '코발트 비대칭 넥 리브 니트', '상의', 'female', 'edgy',
   '코발트', 'medium', '중간', '높음', '베이직', '중간', '포인트', 2, 'slim', 'knit',
   '니트', 'ribbed wool-blend knit, asymmetric neckline cut lower on one side, long sleeves, fitted'),
  ('0ffd0c00-0000-0000-0000-000000000003', '블랙 스트럭처드 숄더 하이넥 탑', '상의', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '포인트', 3, 'slim', 'jersey',
   '긴팔탑', 'firm ponte jersey, structured padded shoulders, high neck, long sleeves, fitted body'),
  ('0ffd0c00-0000-0000-0000-000000000004', '화이트 비대칭 랩 포플린 셔츠', '상의', 'female', 'edgy',
   '화이트', 'thin', '밝음', '낮음', '베이직', '가벼움', '약한포인트', 3, 'regular', 'cotton',
   '셔츠', 'crisp cotton poplin, asymmetric wrap front fastened at one side, sharp pointed collar, long sleeves'),
  ('0ffd0c00-0000-0000-0000-000000000005', '블랙 비대칭 스트랩 탱크탑', '상의', 'female', 'edgy',
   '블랙', 'thin', '어두움', '낮음', '베이직', '가벼움', '약한포인트', 2, 'slim', 'cotton',
   '탱크탑', 'ribbed cotton, asymmetric neckline with one wide and one narrow strap, fitted'),

  -- 하의 — 레더 미디·배럴 레그·크로스오버 데님·비대칭 헴 미니. 배럴 레그는 실루엣을
  -- 잡는 구조템, 허리만 엇갈리는 데님은 베이스다.
  ('0ffd0c00-0000-0000-0000-000000000006', '블랙 레더 미디 스커트', '하의', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '약한포인트', 3, 'slim', 'leather',
   '스커트', 'smooth matte leather, straight cut, midi length, back slit'),
  ('0ffd0c00-0000-0000-0000-000000000007', '차콜 울 배럴 레그 팬츠', '하의', 'female', 'edgy',
   '차콜', 'medium', '어두움', '낮음', '베이직', '중간', '구조템', 3, 'relaxed', 'wool',
   '와이드팬츠', 'wool blend, softly curved barrel leg that narrows to a clean open hem at the ankle bone, no elastic, cuff or gathering at the hem, front pleats, high waist, ankle length'),
  ('0ffd0c00-0000-0000-0000-000000000008', '그레이 워시드 크로스오버 웨이스트 데님', '하의', 'female', 'edgy',
   '그레이', 'medium', '중간', '낮음', '베이직', '중간', '베이스', 2, 'relaxed', 'denim',
   '데님팬츠', 'washed grey denim, asymmetric crossover waistband, wide straight leg, full length'),
  ('0ffd0c00-0000-0000-0000-000000000009', '올리브 비대칭 헴 랩 미니스커트', '하의', 'female', 'edgy',
   '올리브', 'medium', '중간', '중간', '베이직', '중간', '약한포인트', 2, 'slim', 'cotton',
   '스커트', 'cotton twill, wrap front, asymmetric hem, mini length'),

  -- 아우터 — 안감이 없는 레더 블레이저·박시 크롭 재킷(봄·가을), 보온 안감이 있는 롱코트와
  -- 시어링 재킷(겨울). 두께는 안감까지 본 값이다.
  ('0ffd0c00-0000-0000-0000-00000000000a', '브라운 레더 오버사이즈 블레이저', '아우터', 'female', 'edgy',
   '다크브라운', 'medium', '어두움', '중간', '베이직', '무거움', '포인트', 3, 'oversized', 'leather',
   '블레이저', 'matte lambskin leather, structured shoulders, oversized single-breasted cut, hip-length'),
  ('0ffd0c00-0000-0000-0000-00000000000b', '블랙 박시 크롭 울 재킷', '아우터', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '구조템', 3, 'regular', 'wool',
   '크롭자켓', 'dense wool twill, boxy cropped cut, sculpted shoulders, concealed zip front'),
  ('0ffd0c00-0000-0000-0000-00000000000c', '차콜 비대칭 여밈 롱 울 코트', '아우터', 'female', 'edgy',
   '차콜', 'thick', '어두움', '낮음', '베이직', '중간', '구조템', 3, 'regular', 'wool',
   '롱코트', 'heavy wool, structured shoulders, asymmetric overlapping front with off-centre buttons, mid-calf length, insulated lining'),
  ('0ffd0c00-0000-0000-0000-00000000000d', '다크브라운 스웨이드 시어링 에비에이터 재킷', '아우터', 'female', 'edgy',
   '다크브라운', 'thick', '어두움', '중간', '베이직', '무거움', '포인트', 2, 'relaxed', 'suede',
   '시어링자켓', 'brushed suede shell, thick shearling lining, wide shearling collar with strap, asymmetric zip front, hip-length'),

  -- 신발 — 컴뱃 부츠·색이 있는 앵클부츠·여름의 청키 피셔맨 샌들.
  ('0ffd0c00-0000-0000-0000-00000000000e', '블랙 레더 컴뱃 부츠', '신발', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '무거움', '구조템', 2, NULL, 'leather',
   '앵클부츠', 'smooth leather, lace-up front, chunky lug sole, ankle-high'),
  ('0ffd0c00-0000-0000-0000-00000000000f', '버건디 스퀘어토 레더 앵클부츠', '신발', 'female', 'edgy',
   '버건디', 'medium', '어두움', '중간', '베이직', '중간', '약한포인트', 3, NULL, 'leather',
   '앵클부츠', 'polished leather, square toe, sharp block heel, ankle-high'),
  ('0ffd0c00-0000-0000-0000-000000000010', '블랙 레더 청키 피셔맨 샌들', '신발', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '연결템', 2, NULL, 'leather',
   '샌들', 'matte leather, woven fisherman straps, open sides, chunky lug sole'),

  -- 가방
  ('0ffd0c00-0000-0000-0000-000000000011', '블랙 레더 지오메트릭 숄더백', '가방', 'female', 'edgy',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '약한포인트', 3, NULL, 'leather',
   '숄더백', 'matte leather, angular trapezoid shape, rigid body, short strap'),
  ('0ffd0c00-0000-0000-0000-000000000012', '버건디 레더 박스 크로스백', '가방', 'female', 'edgy',
   '버건디', 'medium', '어두움', '중간', '베이직', '중간', '약한포인트', 3, NULL, 'leather',
   '크로스백', 'smooth leather, compact box shape, adjustable crossbody strap')
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
  ('0ffd0c00-0000-0000-0000-000000000001', '봄'), ('0ffd0c00-0000-0000-0000-000000000001', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000002', '가을'), ('0ffd0c00-0000-0000-0000-000000000002', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000003', '봄'), ('0ffd0c00-0000-0000-0000-000000000003', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000003', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000004', '봄'), ('0ffd0c00-0000-0000-0000-000000000004', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000004', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000005', '봄'), ('0ffd0c00-0000-0000-0000-000000000005', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000006', '봄'), ('0ffd0c00-0000-0000-0000-000000000006', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000006', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000007', '봄'), ('0ffd0c00-0000-0000-0000-000000000007', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000007', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000008', '봄'), ('0ffd0c00-0000-0000-0000-000000000008', '가을'),
  ('0ffd0c00-0000-0000-0000-000000000008', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000009', '봄'), ('0ffd0c00-0000-0000-0000-000000000009', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000009', '가을'),
  ('0ffd0c00-0000-0000-0000-00000000000a', '봄'), ('0ffd0c00-0000-0000-0000-00000000000a', '가을'),
  ('0ffd0c00-0000-0000-0000-00000000000b', '봄'), ('0ffd0c00-0000-0000-0000-00000000000b', '가을'),
  ('0ffd0c00-0000-0000-0000-00000000000c', '가을'), ('0ffd0c00-0000-0000-0000-00000000000c', '겨울'),
  ('0ffd0c00-0000-0000-0000-00000000000d', '가을'), ('0ffd0c00-0000-0000-0000-00000000000d', '겨울'),
  ('0ffd0c00-0000-0000-0000-00000000000e', '봄'), ('0ffd0c00-0000-0000-0000-00000000000e', '가을'),
  ('0ffd0c00-0000-0000-0000-00000000000e', '겨울'),
  ('0ffd0c00-0000-0000-0000-00000000000f', '가을'), ('0ffd0c00-0000-0000-0000-00000000000f', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000010', '봄'), ('0ffd0c00-0000-0000-0000-000000000010', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000011', '봄'), ('0ffd0c00-0000-0000-0000-000000000011', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000011', '가을'), ('0ffd0c00-0000-0000-0000-000000000011', '겨울'),
  ('0ffd0c00-0000-0000-0000-000000000012', '봄'), ('0ffd0c00-0000-0000-0000-000000000012', '여름'),
  ('0ffd0c00-0000-0000-0000-000000000012', '가을'), ('0ffd0c00-0000-0000-0000-000000000012', '겨울');

-- ─── 텍스처 ───
-- 어휘는 Vision 프롬프트의 규칙(`prompts::texture_world_rule`)을 따른다. 맞는 값이 없는
-- 저지·니트 상의는 비워 둔다.
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd0c00-0000-0000-0000-000000000006', 'leather'),
  ('0ffd0c00-0000-0000-0000-000000000007', 'tailoring'),
  ('0ffd0c00-0000-0000-0000-000000000008', 'denim'),
  ('0ffd0c00-0000-0000-0000-00000000000a', 'leather'),
  ('0ffd0c00-0000-0000-0000-00000000000a', 'tailoring'),
  ('0ffd0c00-0000-0000-0000-00000000000c', 'tailoring'),
  ('0ffd0c00-0000-0000-0000-00000000000e', 'leather'),
  ('0ffd0c00-0000-0000-0000-00000000000f', 'leather'),
  ('0ffd0c00-0000-0000-0000-000000000010', 'leather'),
  ('0ffd0c00-0000-0000-0000-000000000011', 'leather'),
  ('0ffd0c00-0000-0000-0000-000000000012', 'leather');

-- ─── 장르 태그 ───
-- 새 옷 18벌과, 형태가 있는 옷을 받쳐 줄 기존 옷 17벌. 기존 옷의 원래 태그는 그대로다.
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, 'edgy', 'seed' FROM clothing
WHERE id LIKE '0ffd0c00-0000-0000-0000-%'
   OR id IN (
     -- 아우터: 라이더는 여럿 중 하나다. 크롭 데님·각진 블레이저·보온 안감 시어링을 같이 둔다.
     '0ffd0700-0000-0000-0000-000000000008', -- 블랙 레더 라이더 자켓
     'd892bf4a-546b-11f1-af43-264d116ef584', -- 워시드 블랙 크롭 데님자켓
     '0ffd0a00-0000-0000-0000-000000000000', -- 차콜 울 오버사이즈 블레이저
     '0ffd0b00-0000-0000-0000-000000000012', -- 블랙 레더 시어링 재킷
     -- 상의: 형태가 있는 하의·아우터 아래에 받칠 몸에 맞는 니트와 기본 탑.
     'd8929e84-546b-11f1-af43-264d116ef584', -- 블랙 리브드 슬림 니트
     '8e2f79ba-598a-11f1-af43-264d116ef584', -- 그레이 니트 터틀넥 탑
     '0ffd0700-0000-0000-0000-000000000003', -- 블랙 리브 탱크탑
     '0ffd0700-0000-0000-0000-000000000000', -- 화이트 코튼 크루넥 티셔츠
     -- 하의
     '0ffd0700-0000-0000-0000-000000000006', -- 워시드 블랙 슬림 데님
     '0ffd0800-0000-0000-0000-000000000007', -- 인디고 와이드 데님
     'd892aeb0-546b-11f1-af43-264d116ef584', -- 블랙 와이드 슬랙스
     '0ffd0a00-0000-0000-0000-000000000002', -- 차콜 울 와이드 슬랙스
     -- 신발
     '0ffd0700-0000-0000-0000-00000000000c', -- 블랙 레더 앵클부츠
     '0ffd0800-0000-0000-0000-000000000003', -- 블랙 레더 첼시부츠
     '0ffd0b00-0000-0000-0000-000000000009', -- 블랙 레더 니하이 롱부츠
     -- 가방
     '8e2f841e-598a-11f1-af43-264d116ef584', -- 블랙 버클 스트랩 숄더백
     'd892b450-546b-11f1-af43-264d116ef584'  -- 블랙 레더 미니 숄더백
   );
