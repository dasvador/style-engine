-- 여성 '모델 오프듀티' 아이템 시드.
--
-- 이 장르는 후보가 0건이다. 남성 장르를 점검하다 같은 방식으로 드러났다.
--
-- 경위는 `20260910000004` 주석에 이미 적혀 있다. 예전 `off_duty` 로 태깅된 21벌이
-- 전부 `style = '스포츠'`(스포츠브라·레깅스·러닝화·조거팬츠)였고, 그 마이그레이션이
-- 판단 기준을 이름이 아니라 `style` 컬럼에 두면서 21벌을 모두 `sporty_casual` 로
-- 옮겼다. 그 판단 자체는 옳다 — 레깅스는 모델 사복이 아니다. 다만 옮기고 나니
-- 새로 만든 '모델 오프듀티' 칩에 남은 아이템이 하나도 없게 됐다.
--
-- 이 장르의 정의(데님과 티셔츠 같은 기본 아이템에 힘을 뺀 자연스러운 스타일)에
-- 해당하는 옷이 여성 옷장에 실제로 없다. 조건으로 메울 수 없어 아이템을 넣는다.
--
-- id 를 고정해 두어 재실행에 안전하다.
--
-- 계절(`clothing_season`)을 넣지 않는다. 남성 아이템은 140벌 전부 계절이 붙어
-- 있지만 여성은 141벌 전부 비어 있고, 계절 게이트는 값이 없는 슬롯을 아예 건너뛴다
-- (`style_engine_v2::detect_season_complete_mismatch`, `score_utility` 의
-- `if slot.seasons.is_empty() { continue; }`). 즉 지금 여성 옷장에서 계절 게이트는
-- 작동하지 않는다.
--
-- 여기서만 계절을 넣으면 이 장르만 게이트가 켜진다. 모델 오프듀티는 아이템 공급원이
-- 이 시드뿐이라 코디가 전부 이 아이템으로 구성되고, `total >= 3` 조건이 채워져
-- 겨울에 하드 필터로 떨어질 수 있다 — 다른 여성 장르는 그대로 통과하는데 이 장르만
-- 그렇게 된다. 그래서 기존 여성 데이터와 같은 상태로 둔다.
--
-- 여성 아이템 전체에 계절을 채우는 것은 별개의 데이터 작업이다.

INSERT INTO clothing
  (id, name, category, gender, style_mood, color, thickness, tone, saturation, style,
   weight, role, formality_level, material_primary, sub_category, texture_keywords)
VALUES
  -- 상의 — 힘을 뺀 기본 아이템이 이 장르의 중심이다.
  ('0ffd0700-0000-0000-0000-000000000000', '화이트 코튼 크루넥 티셔츠', '상의', 'female', 'model_off_duty',
   'white', 'thin', '밝음', '낮음', '베이직', '가벼움', '베이스', 1, 'cotton', '반팔티셔츠',
   'cotton jersey, relaxed crewneck'),
  ('0ffd0700-0000-0000-0000-000000000001', '헤더그레이 오버사이즈 티셔츠', '상의', 'female', 'model_off_duty',
   'heather_gray', 'thin', '중간', '낮음', '베이직', '가벼움', '베이스', 1, 'cotton', '반팔티셔츠',
   'cotton jersey, oversized, dropped shoulder'),
  ('0ffd0700-0000-0000-0000-000000000002', '네이비 스트라이프 롱슬리브', '상의', 'female', 'model_off_duty',
   'navy', 'thin', '어두움', '낮음', '베이직', '가벼움', '베이스', 1, 'cotton', '긴팔티셔츠',
   'cotton jersey, breton stripe'),
  ('0ffd0700-0000-0000-0000-000000000003', '블랙 리브 탱크탑', '상의', 'female', 'model_off_duty',
   'black', 'thin', '어두움', '낮음', '베이직', '가벼움', '베이스', 1, 'cotton', '탱크탑',
   'ribbed cotton, fitted tank'),
  ('0ffd0700-0000-0000-0000-000000000004', '오트밀 코튼 니트 크루넥', '상의', 'female', 'model_off_duty',
   'oatmeal', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 2, 'knit', '니트',
   'cotton knit, relaxed crewneck'),

  -- 하의 — 데님이 이 장르의 기본 축이다.
  ('0ffd0700-0000-0000-0000-000000000005', '라이트 인디고 스트레이트 데님', '하의', 'female', 'model_off_duty',
   'light_indigo', 'medium', '중간', '중간', '베이직', '중간', '베이스', 1, 'denim', '데님팬츠',
   'denim, straight leg, light wash'),
  ('0ffd0700-0000-0000-0000-000000000006', '워시드 블랙 슬림 데님', '하의', 'female', 'model_off_duty',
   'washed_black', 'medium', '어두움', '낮음', '베이직', '중간', '베이스', 1, 'denim', '데님팬츠',
   'denim, slim leg, washed black'),
  ('0ffd0700-0000-0000-0000-000000000007', '크림 코튼 와이드 팬츠', '하의', 'female', 'model_off_duty',
   'cream', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 2, 'cotton', '와이드팬츠',
   'cotton twill, wide leg'),

  -- 아우터 — 가죽·데님 재킷을 툭 걸치는 구성.
  ('0ffd0700-0000-0000-0000-000000000008', '블랙 레더 라이더 자켓', '아우터', 'female', 'model_off_duty',
   'black', 'medium', '어두움', '낮음', '베이직', '무거움', '포인트', 2, 'leather', '라이더자켓',
   'leather, cropped biker, silver hardware'),
  ('0ffd0700-0000-0000-0000-000000000009', '인디고 데님 트러커 자켓', '아우터', 'female', 'model_off_duty',
   'indigo', 'medium', '어두움', '중간', '베이직', '중간', '연결템', 1, 'denim', '데님자켓',
   'denim, trucker jacket, chest pockets'),

  -- 신발 · 가방
  ('0ffd0700-0000-0000-0000-00000000000a', '화이트 레더 로우 스니커즈', '신발', 'female', 'model_off_duty',
   'white', 'medium', '밝음', '낮음', '베이직', '중간', '베이스', 1, 'leather', '스니커즈',
   'leather, minimal low-top'),
  ('0ffd0700-0000-0000-0000-00000000000b', '브라운 레더 토트백', '가방', 'female', 'model_off_duty',
   'brown', 'medium', '중간', '낮음', '베이직', '중간', '약한포인트', 2, 'leather', '토트백',
   'leather, soft unstructured tote'),
  ('0ffd0700-0000-0000-0000-00000000000c', '블랙 레더 앵클부츠', '신발', 'female', 'model_off_duty',
   'black', 'medium', '어두움', '낮음', '베이직', '중간', '연결템', 2, 'leather', '앵클부츠',
   'leather, block heel ankle boot'),
  ('0ffd0700-0000-0000-0000-00000000000d', '탄 스웨이드 로퍼', '신발', 'female', 'model_off_duty',
   'tan', 'medium', '중간', '낮음', '베이직', '중간', '베이스', 2, 'suede', '로퍼',
   'suede, flat loafer')
ON DUPLICATE KEY UPDATE id = clothing.id;
