-- 남녀 무드별 방한 외투 데모 시드. 개발·데모 DB(default 옷장) 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/winter_outer.sql
--   python3 tools/demo-seed/apply.py seeds/demo/winter_outer_rollback.sql   # 되돌리기
--
-- 왜: 2026-11-18 영하(-5/-10°C) 실측 기준선에서 남성 8무드는 겨울 외투(두꺼움 + 보온 안감·충전재
-- 확인, `serving_ranker::is_winter_outer`)가 0벌이었고, 기준선 남성 91장의 외투는 코치·하링턴
-- 재킷이나 안감 미확인 울 코트 같은 겨울 외투가 아닌 옷이었다. 여성은 미니멀·클래식·로맨틱·모던시크·보헤미안·
-- 매니시·스포티가 0벌, 스트리트가 1벌이었다.
--
-- 무엇: 무드마다 겨울 외투 2벌(여성 스트리트는 1벌 추가), 남 16 · 여 15 = 31벌. 새 옷은 자기 무드
-- 태그 하나만 갖는다. 한 무드 안의 두 벌은 색이 다르다. 무드 사이에는 색·소재가 겹치는 옷도 있다.
--
-- 근거: 무드별로 (A) 셀럽·스트리트 착용 사실, (B) 실제 제품 페이지의 겉감·보온 사양, (C) 가상 샘플
-- 해석을 나눠 `winter_outer_sources.md` 에 적었다. 여기 옷은 전부 (C) 가상 샘플이다 — 착용자가
-- 입은 그 옷이 아니고, 색·기장은 해석이다. 보온 낱말(insulated, down fill, quilted, shearling
-- lining …)은 (B) 에서 옮기려 한 것이며, 줄별 대조는 그 문서를 본다.
--
-- 기존 옷은 이름 한 건만 고친다(아래). 다른 기존 옷의 이름·속성은 건드리지 않는다. 이름으로 안감·
-- 소재를 추측해 바꾸지도 않는다 — '올리브 퀼팅 자켓'·'올리브 라이너 자켓'은 안감 미확인으로 둔다.
--
-- 범위: 대상은 아래 `winter_outer_ids` 의 31개 id 중 user_id = 'default' 인 행뿐이다. 같은 id 를
-- 다른 사용자가 이미 갖고 있으면 그 행은 바꾸지도 넘겨받지도 않고, 계절·태그도 붙이지 않는다.
-- 계절·태그는 default 소유 행에만 JOIN 해서 붙인다.
--
-- 재실행 안전: 한 트랜잭션. default 소유의 같은 id 가 있으면 시드 값으로 맞추고, 계절·태그는
-- 이미 있으면 건너뛴다. 이름 보정은 깨진 이름 그대로일 때만 바뀐다.

START TRANSACTION;

DROP TEMPORARY TABLE IF EXISTS winter_outer_ids;
CREATE TEMPORARY TABLE winter_outer_ids AS SELECT id FROM clothing WHERE 1 = 0;
INSERT INTO winter_outer_ids (id) VALUES
  ('0ffd1100-0000-0000-0000-000000000101'),
  ('0ffd1100-0000-0000-0000-000000000102'),
  ('0ffd1100-0000-0000-0000-000000000103'),
  ('0ffd1100-0000-0000-0000-000000000104'),
  ('0ffd1100-0000-0000-0000-000000000105'),
  ('0ffd1100-0000-0000-0000-000000000106'),
  ('0ffd1100-0000-0000-0000-000000000107'),
  ('0ffd1100-0000-0000-0000-000000000108'),
  ('0ffd1100-0000-0000-0000-000000000109'),
  ('0ffd1100-0000-0000-0000-000000000110'),
  ('0ffd1100-0000-0000-0000-000000000111'),
  ('0ffd1100-0000-0000-0000-000000000112'),
  ('0ffd1100-0000-0000-0000-000000000113'),
  ('0ffd1100-0000-0000-0000-000000000114'),
  ('0ffd1100-0000-0000-0000-000000000115'),
  ('0ffd1100-0000-0000-0000-000000000116'),
  ('0ffd1100-0000-0000-0000-000000000201'),
  ('0ffd1100-0000-0000-0000-000000000202'),
  ('0ffd1100-0000-0000-0000-000000000203'),
  ('0ffd1100-0000-0000-0000-000000000204'),
  ('0ffd1100-0000-0000-0000-000000000205'),
  ('0ffd1100-0000-0000-0000-000000000206'),
  ('0ffd1100-0000-0000-0000-000000000207'),
  ('0ffd1100-0000-0000-0000-000000000208'),
  ('0ffd1100-0000-0000-0000-000000000209'),
  ('0ffd1100-0000-0000-0000-000000000210'),
  ('0ffd1100-0000-0000-0000-000000000211'),
  ('0ffd1100-0000-0000-0000-000000000212'),
  ('0ffd1100-0000-0000-0000-000000000213'),
  ('0ffd1100-0000-0000-0000-000000000214'),
  ('0ffd1100-0000-0000-0000-000000000215');

-- ─── 깨진 이름 복원(남성 아우터 1건) ───
-- 현재 이름은 '\uFFFD\uFFFD콜 울 오버셔츠'(아래 HEX). tests/fixtures/wardrobe_registry.toml 의
-- `charcoal_wool_overshirt`(name='차콜 울 오버셔츠', category='아우터')와 남은 글자 '콜 울 오버셔츠'가
-- 맞고, 같은 행 color='차콜' 이 깨진 두 글자와 맞는다. 픽스처에는 id 가 없어 id 로는 대조하지 못했다.
UPDATE clothing SET name = '차콜 울 오버셔츠'
 WHERE id = 'db0ac4d9-da28-4c13-9761-35e1cabd9093' AND user_id = 'default'
   AND category = '아우터' AND color = '차콜'
   AND HEX(name) = 'EFBFBDEFBFBDECBD9C20EC9AB820EC98A4EBB284EC8594ECB8A0';

-- ─── 새 겨울 외투(가상 샘플) ───
INSERT INTO clothing
  (id, user_id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, color_temperature, formality_level, silhouette_volume, material_primary,
   sub_category, texture_keywords)
VALUES
  -- ── 남성 ──
  -- 미니멀
  ('0ffd1100-0000-0000-0000-000000000101', 'default', '라이트브라운 멜란지 패디드 오버사이즈 울 코트', '아우터', 'male', 'minimal',
   '라이트브라운', 'thick', '중간', '낮음', '베이직', '무거움', '구조템', 'warm', 3, 'oversized', 'wool', 'coat',
   'soft melange wool, oversized single-breasted, polyester padded filling, cupro lining, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000102', 'default', '블랙 다운 패디드 테일러드 코트', '아우터', 'male', 'minimal',
   '블랙', 'thick', '어두움', '낮음', '베이직', '중간', '구조템', 'neutral', 3, 'regular', 'polyester', 'coat',
   'matte recycled polyester shell, notch-lapel tailored coat shape, down fill 80/20 in flat channels, thigh-length'),
  -- 클래식
  ('0ffd1100-0000-0000-0000-000000000103', 'default', '그레이 헤링본 패디드 퀼트 라이닝 코트', '아우터', 'male', 'classic',
   '그레이', 'thick', '중간', '낮음', '포멀', '무거움', '구조템', 'neutral', 3, 'regular', 'wool', 'coat',
   'wool herringbone, single-breasted overcoat, padded quilted lining, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000104', 'default', '초콜릿 스웨이드 시어링 더블 코트', '아우터', 'male', 'classic',
   '초콜릿브라운', 'thick', '어두움', '낮음', '베이직', '무거움', '포인트', 'warm', 3, 'regular', 'suede', 'coat',
   'lambswool suede, double-breasted, cream shearling lining and collar, thigh-length'),
  -- 프레피
  ('0ffd1100-0000-0000-0000-000000000105', 'default', '에크루 더블페이스 울 퀼티드 라이닝 더플코트', '아우터', 'male', 'preppy',
   '에크루', 'thick', '밝음', '낮음', '베이직', '무거움', '구조템', 'warm', 2, 'regular', 'wool', 'coat',
   'double-faced wool, hooded duffle with horn toggles, quilted lining, thigh-length'),
  ('0ffd1100-0000-0000-0000-000000000106', 'default', '포레스트그린 타탄 안감 배플 퀼팅 재킷', '아우터', 'male', 'preppy',
   '포레스트그린', 'thick', '어두움', '중간', '베이직', '중간', '연결템', 'neutral', 2, 'regular', 'polyester', 'jacket',
   'baffle-quilted polyester shell, fibre down filled baffles, brushed cotton tartan lining, cord collar, hip-length'),
  -- 아메카지
  ('0ffd1100-0000-0000-0000-000000000107', 'default', '블랙 레더 아이보리 시어링 B-3 재킷', '아우터', 'male', 'amekaji',
   '블랙/아이보리', 'thick', '어두움', '낮음', '밀리터리', '무거움', '포인트', 'neutral', 2, 'oversized', 'shearling', 'bomber',
   'black leather face sheepskin, natural ivory shearling lining and wide collar, collar straps, hip-length'),
  ('0ffd1100-0000-0000-0000-000000000108', 'default', '탄 코듀로이 시어링 칼라 랜치 코트', '아우터', 'male', 'amekaji',
   '탄', 'thick', '중간', '낮음', '워크', '무거움', '연결템', 'warm', 2, 'regular', 'corduroy', 'jacket',
   'cotton corduroy ranch coat, sheep shearling collar, insulated polyester fill, hip-covering length'),
  -- 워크웨어
  ('0ffd1100-0000-0000-0000-000000000109', 'default', '토바코 캔버스 블랭킷 라이닝 디트로이트 재킷', '아우터', 'male', 'workwear',
   '토바코브라운', 'thick', '중간', '낮음', '워크', '무거움', '구조템', 'warm', 1, 'relaxed', 'canvas', 'jacket',
   'rigid cotton duck canvas, corduroy collar, striped blanket lining, nylon taffeta sleeve lining, boxy, hip-length'),
  ('0ffd1100-0000-0000-0000-000000000110', 'default', '블랙 다이아몬드 퀼팅 캔버스 아틱 코트', '아우터', 'male', 'workwear',
   '블랙', 'thick', '어두움', '낮음', '워크', '무거움', '구조템', 'neutral', 1, 'relaxed', 'canvas', 'coat',
   'heavy cotton duck canvas, corduroy collar, diamond quilted lining, loose fit, thigh-length'),
  -- 스트리트
  ('0ffd1100-0000-0000-0000-000000000111', 'default', '옥스블러드 패디드 램레더 코트', '아우터', 'male', 'street',
   '옥스블러드', 'thick', '어두움', '중간', '베이직', '무거움', '포인트', 'warm', 2, 'relaxed', 'leather', 'coat',
   'soft lamb leather, thin padded quilting, cupro lining, relaxed car-coat cut, thigh-length'),
  ('0ffd1100-0000-0000-0000-000000000112', 'default', '차콜 페이크퍼 후드 패디드 봄버', '아우터', 'male', 'street',
   '차콜', 'thick', '어두움', '낮음', '밀리터리', '무거움', '포인트', 'neutral', 1, 'oversized', 'nylon', 'bomber',
   'nylon-poly shell, padded polyester filling, nylon lining, black faux-fur trimmed hood, oversized, hip-length'),
  -- 스포티 캐주얼
  ('0ffd1100-0000-0000-0000-000000000113', 'default', '하이퍼로열 프리마로프트 후디드 패딩 재킷', '아우터', 'male', 'sporty_casual',
   '하이퍼로열블루', 'thick', '중간', '높음', '스포츠', '중간', '포인트', 'cool', 1, 'relaxed', 'nylon', 'parka',
   'nylon shell, PrimaLoft insulated fill, polyester lining, loose hooded cut, hip-length'),
  ('0ffd1100-0000-0000-0000-000000000114', 'default', '크림 리버서블 패디드 트랙 봄버', '아우터', 'male', 'sporty_casual',
   '크림', 'thick', '밝음', '낮음', '스포츠', '중간', '연결템', 'warm', 1, 'relaxed', 'polyester', 'bomber',
   'recycled polyester shell, padded polyester insulation, reversible, ribbed track collar, hip-length'),
  -- 아웃도어 캐주얼
  ('0ffd1100-0000-0000-0000-000000000115', 'default', '번트오렌지 구스다운 후디드 재킷', '아우터', 'male', 'outdoor_casual',
   '번트오렌지', 'thick', '중간', '높음', '스포츠', '중간', '포인트', 'warm', 1, 'regular', 'nylon', 'parka',
   'ripstop nylon shell, 750-fill goose down fill, hood, hip-length'),
  ('0ffd1100-0000-0000-0000-000000000116', 'default', '스톤베이지 코어로프트 인슐레이티드 후디', '아우터', 'male', 'outdoor_casual',
   '스톤베이지', 'thick', '밝음', '낮음', '스포츠', '중간', '연결템', 'neutral', 1, 'regular', 'nylon', 'jacket',
   'tightly woven nylon shell, Coreloft synthetic insulated, fleece-lined hood, hip-length'),

  -- ── 여성 ──
  -- 미니멀
  ('0ffd1100-0000-0000-0000-000000000201', 'default', '비스킷 컬리 시어링 레더 트림 롱코트', '아우터', 'female', 'minimal',
   '비스킷', 'thick', '밝음', '낮음', '베이직', '무거움', '구조템', 'warm', 3, 'relaxed', 'shearling', '롱코트',
   'heavyweight curly lamb shearling, smooth leather collar and placket trim, collarless clean front, mid-calf'),
  ('0ffd1100-0000-0000-0000-000000000202', 'default', '블랙 펠티드 울 퀼트 안감 재킷', '아우터', 'female', 'minimal',
   '블랙', 'thick', '어두움', '낮음', '베이직', '중간', '구조템', 'neutral', 3, 'regular', 'wool', '자켓',
   'felted wool-cotton, lightly padded quilted lining, boxy collarless jacket, hip-length'),
  -- 클래식
  ('0ffd1100-0000-0000-0000-000000000203', 'default', '카멜 캐멀울 리버서블 다운 코트', '아우터', 'female', 'classic',
   '카멜', 'thick', '중간', '낮음', '포멀', '무거움', '구조템', 'warm', 3, 'regular', 'wool', '코트',
   'camel wool face, reversible to quilted side, goose down fill quilting, belted wrap, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000204', 'default', '네이비 울 다이아몬드 퀼트 안감 피코트', '아우터', 'female', 'classic',
   '네이비', 'thick', '어두움', '낮음', '포멀', '무거움', '구조템', 'cool', 3, 'regular', 'wool', '코트',
   'wool-blend melton, double-breasted peacoat, polyester fill diamond quilted lining, high collar, thigh-length'),
  -- 로맨틱
  ('0ffd1100-0000-0000-0000-000000000205', 'default', '버터크림 리본 아플리케 페이크퍼 안감 후드 코트', '아우터', 'female', 'romantic',
   '버터크림', 'thick', '밝음', '낮음', '베이직', '중간', '포인트', 'warm', 2, 'relaxed', 'polyester', '코트',
   'water-repellent shell, faux fur lining, padded polyester filling, hood, crossover front with bow appliques, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000206', 'default', '파우더핑크 플레어 헴 벨티드 다운 코트', '아우터', 'female', 'romantic',
   '파우더핑크', 'thick', '밝음', '중간', '베이직', '중간', '포인트', 'warm', 2, 'slim', 'down', '패딩',
   'matte shell, down fill, fitted waist with tie belt, flared skirt-like hem, stand collar, knee-length'),
  -- 모던 시크
  ('0ffd1100-0000-0000-0000-000000000207', 'default', '블랙 하이넥 슬림 롱 다운 코트', '아우터', 'female', 'modern_chic',
   '블랙', 'thick', '어두움', '낮음', '베이직', '무거움', '구조템', 'neutral', 3, 'slim', 'down', '롱코트',
   'sleek matte shell, down fill, high stand collar, slim straight column, two-way zip, mid-calf'),
  ('0ffd1100-0000-0000-0000-000000000208', 'default', '초콜릿 레더 시어링 라펠 후드 코트', '아우터', 'female', 'modern_chic',
   '초콜릿브라운', 'thick', '어두움', '낮음', '베이직', '무거움', '포인트', 'warm', 3, 'relaxed', 'leather', '롱코트',
   'smooth lambskin leather, off-white shearling lining, wide shearling lapels and cuffs, oversized hood, below the knee'),
  -- 보헤미안
  ('0ffd1100-0000-0000-0000-000000000209', 'default', '탄 페이크 스웨이드 페니레인 코트', '아우터', 'female', 'bohemian',
   '탄', 'thick', '중간', '낮음', '베이직', '무거움', '포인트', 'warm', 2, 'relaxed', 'suede', '코트',
   'faux suede shell, fleece lining, shaggy cream faux shearling collar cuffs and hem, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000210', 'default', '테라코타 울 이펙트 프로그 여밈 시어링 안감 코트', '아우터', 'female', 'bohemian',
   '테라코타', 'thick', '중간', '중간', '베이직', '무거움', '포인트', 'warm', 2, 'relaxed', 'wool', '코트',
   'wool-effect shell, faux shearling interior lining, padded polyester filling, frog fastenings, shirt collar, knee-length'),
  -- 매니시
  ('0ffd1100-0000-0000-0000-000000000211', 'default', '그레이 헤링본 패디드 오버사이즈 코트', '아우터', 'female', 'mannish',
   '그레이', 'thick', '중간', '낮음', '포멀', '무거움', '구조템', 'neutral', 3, 'oversized', 'wool', '롱코트',
   'wool-nylon herringbone, padded quilted lining, dropped shoulder, single-breasted, knee-length'),
  ('0ffd1100-0000-0000-0000-000000000212', 'default', '카키 워시드 코튼 페이크 시어링 안감 숏 코트', '아우터', 'female', 'mannish',
   '카키', 'thick', '중간', '낮음', '워크', '무거움', '연결템', 'warm', 2, 'relaxed', 'cotton', '코트',
   'washed cotton shell, contrast faux shearling lining, padded polyester filling, snap placket, patch flap pocket, hip-length'),
  -- 스포티 캐주얼
  ('0ffd1100-0000-0000-0000-000000000213', 'default', '아이스블루 페이크 시어링 트림 다운 스키 재킷', '아우터', 'female', 'sporty_casual',
   '아이스블루', 'thick', '밝음', '중간', '스포츠', '중간', '포인트', 'cool', 1, 'relaxed', 'down', '패딩',
   'nylon ski shell, goose down fill 90/10, cotton lining, faux shearling trim on collar and seams, hip-length'),
  ('0ffd1100-0000-0000-0000-000000000214', 'default', '로덴그린 프리마로프트 퍼퍼 봄버', '아우터', 'female', 'sporty_casual',
   '로덴그린', 'thick', '어두움', '중간', '스포츠', '중간', '연결템', 'neutral', 1, 'regular', 'nylon', '패딩',
   'nylon shell, PrimaLoft insulated fill, ribbed bomber collar and cuffs, hip-length'),
  -- 스트리트(기존 1벌 + 1)
  ('0ffd1100-0000-0000-0000-000000000215', 'default', '차콜 페이크퍼 후드 오버사이즈 파카', '아우터', 'female', 'street',
   '차콜', 'thick', '어두움', '낮음', '밀리터리', '무거움', '구조템', 'neutral', 1, 'oversized', 'polyester', '코트',
   'polyester shell, padded polyester filling, faux fur hood and cuffs, elasticated waist, oversized, thigh-length')
AS new
ON DUPLICATE KEY UPDATE
  -- user_id 는 바꾸지 않는다. 다른 사용자 소유 행이면 모든 칸을 그대로 둔다.
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

-- 계절: 전부 겨울. default 소유 대상 행에만.
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, '겨울'
  FROM winter_outer_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default';

-- 장르 태그: 자기 무드 하나만. default 소유 대상 행에만.
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT c.id, c.style_mood, 'seed'
  FROM winter_outer_ids t
  JOIN clothing c ON c.id = t.id AND c.user_id = 'default';

DROP TEMPORARY TABLE winter_outer_ids;

COMMIT;
