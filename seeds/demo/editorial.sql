-- 여성 엣지·글램·로맨틱의 착장 폭을 넓히는 데모 시드. 개발·데모 DB 전용이다 — 마이그레이션이 아니다.
--
--   python3 tools/demo-seed/apply.py seeds/demo/editorial.sql
--   python3 tools/demo-seed/apply.py seeds/demo/editorial_rollback.sql   # 되돌리기
--
-- 새 무드를 만들지 않는다. 무드 칩은 이미 있다(엣지·글램은 각 시드, 로맨틱은 마이그레이션). 이 시드는
-- 사진 참고(2026-10-18)에서 관찰한 옷의 구조 — 가로 절개 레이어드, 셔링 저지, 레이스 트리밍, 드레이프
-- 맥시 — 를 일반 디자인 요소로 옮긴 새 옷 5벌을 더한다. 특정 제품·로고·프린트는 옮기지 않는다.
--   - 엣지: 가로로 절개한 띠 아래 불투명 안쪽 패널이 붙은 일체형 긴팔. 밝은 색이어도 구조가 엣지다.
--   - 글램: 비대칭 랩넥 레이스 트리밍 셔링 긴팔, 셔링 드레이프 사이드슬릿 맥시스커트, 보라 새틴 레이스
--     가슴단이 붙은 그레이 셔링 튜브탑(퍼 아우터·데님과 입는다 — 퍼는 기존 글램 옷을 쓴다).
--   - 로맨틱: 흰 레이스 안단이 붙은 세이지 스퀘어넥 셔링 긴팔. 꽃무늬·리본이 아닌 로맨틱이다.
-- 사진의 딥 플럼 셔링 저지는 넣지 않았다 — 기존 '버건디 벨벳 비대칭 넥 탑'·'블랙 원숄더 저지 탑'과
-- 역할이 겹친다.
--
-- 기존 옷은 태그만 더한다: 로맨틱에 '워시드 미디엄블루 와이드 데님'(사진의 워싱 데님과 같은 종류).
-- 원래 태그와 값은 고치지 않는다.
--
-- 속성은 실제 옷 그대로다. 얇은 저지는 thin 이고, 레이스·셔링이 있는 상의는 텍스처 'romantic' 이다
-- (업로드 분석 기준과 같다). 일체형인 옷은 설명에 "one-piece" 와 안쪽 패널을 적는다 — 그림이 겹옷이나
-- 지나치게 뚫린 옷으로 그려지지 않게 한다. 원피스·소품·다중 하의는 넣지 않는다.
--
-- 재실행 안전: 새 옷은 고정 id(`0ffd1000-…`). 같은 id 가 있으면 값을 시드대로 맞추고, 계절·텍스처·
-- 태그는 INSERT IGNORE 다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, silhouette_volume, material_primary, sub_category,
   texture_keywords)
VALUES
  -- 엣지
  ('0ffd1000-0000-0000-0000-000000000001', '아이보리 가로 절개 레이어드 저지 롱슬리브', '상의', 'female', 'edgy',
   '아이보리', 'thin', '밝음', '낮음', '베이직', '가벼움', '포인트', 2, 'slim', 'jersey', '긴팔탑',
   'fine matte jersey, one-piece top with a deconstructed front of horizontal slashed jersey strips sewn over an attached opaque ivory inner panel, wide scoop neckline, long sleeves, close fit'),

  -- 글램
  ('0ffd1000-0000-0000-0000-000000000002', '차콜그레이 비대칭 랩넥 레이스 트리밍 셔링 탑', '상의', 'female', 'glam',
   '차콜그레이', 'thin', '어두움', '낮음', '베이직', '가벼움', '포인트', 3, 'slim', 'jersey', '긴팔탑',
   'soft matte stretch jersey, asymmetric wrap neckline edged with fine grey lace, lace-trimmed hem, side ruching, long sleeves, fitted'),
  ('0ffd1000-0000-0000-0000-000000000003', '블랙 셔링 드레이프 사이드슬릿 맥시스커트', '하의', 'female', 'glam',
   '블랙', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 3, 'slim', 'jersey', '롱스커트',
   'matte stretch jersey, draped ruching gathered at one hip, self-fabric tie at the waist, side slit to the knee, ankle-length maxi'),
  ('0ffd1000-0000-0000-0000-000000000004', '퍼플 새틴 레이스 바스트 그레이 셔링 튜브탑', '상의', 'female', 'glam',
   '그레이/퍼플', 'thin', '중간', '낮음', '베이직', '가벼움', '포인트', 3, 'slim', 'jersey', '튜브탑',
   'one-piece strapless top: ruched grey stretch mesh over an opaque lining, attached purple satin bust band edged with dark purple lace, fitted'),

  -- 로맨틱
  ('0ffd1000-0000-0000-0000-000000000005', '세이지 레이스 안단 스퀘어넥 셔링 롱슬리브', '상의', 'female', 'romantic',
   '세이지', 'thin', '밝음', '낮음', '베이직', '가벼움', '베이스', 2, 'slim', 'jersey', '긴팔탑',
   'soft washed jersey, one-piece top with a wide square neckline and a white lace-edged inner layer sewn in at the neckline, side ruching, long sleeves, fitted')
AS new
ON DUPLICATE KEY UPDATE
  name = new.name, category = new.category, gender = new.gender,
  style_mood = new.style_mood, color = new.color, thickness = new.thickness,
  tone = new.tone, saturation = new.saturation, style = new.style, weight = new.weight,
  role = new.role, formality_level = new.formality_level,
  silhouette_volume = new.silhouette_volume, material_primary = new.material_primary,
  sub_category = new.sub_category, texture_keywords = new.texture_keywords;

INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES
  ('0ffd1000-0000-0000-0000-000000000001', '봄'), ('0ffd1000-0000-0000-0000-000000000001', '가을'),
  ('0ffd1000-0000-0000-0000-000000000002', '봄'), ('0ffd1000-0000-0000-0000-000000000002', '가을'),
  ('0ffd1000-0000-0000-0000-000000000003', '봄'), ('0ffd1000-0000-0000-0000-000000000003', '가을'),
  ('0ffd1000-0000-0000-0000-000000000004', '봄'), ('0ffd1000-0000-0000-0000-000000000004', '여름'),
  ('0ffd1000-0000-0000-0000-000000000004', '가을'),
  ('0ffd1000-0000-0000-0000-000000000005', '봄'), ('0ffd1000-0000-0000-0000-000000000005', '가을');

INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES
  ('0ffd1000-0000-0000-0000-000000000002', 'romantic'),
  ('0ffd1000-0000-0000-0000-000000000004', 'romantic'),
  ('0ffd1000-0000-0000-0000-000000000005', 'romantic');

-- ─── 장르 태그 ───
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source) VALUES
  ('0ffd1000-0000-0000-0000-000000000001', 'edgy', 'seed'),
  ('0ffd1000-0000-0000-0000-000000000002', 'glam', 'seed'),
  ('0ffd1000-0000-0000-0000-000000000003', 'glam', 'seed'),
  ('0ffd1000-0000-0000-0000-000000000004', 'glam', 'seed'),
  ('0ffd1000-0000-0000-0000-000000000005', 'romantic', 'seed'),
  -- 기존 옷: 로맨틱 세이지 탑과 입는 워싱 데님(원래 매니시)
  ('d89291be-546b-11f1-af43-264d116ef584', 'romantic', 'seed');
