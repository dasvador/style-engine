-- 홈 추천에서 나온 두 코디를 나란히 두고 "어느 쪽이 더 끌리는지" 를 사람에게 직접 묻는다.
--
-- 지금까지의 반응 쌍(`look_signal`, `look_pair`)은 좋아요·지우기에서 거꾸로 만든 것이라,
-- 무반응이 "못 봤다" 인지 "별로였다" 인지 가르지 못했다. 여기서는 두 코디를 같은 조건으로
-- 보여 주고 A·B·둘 다 아쉬움 중 하나를 받는다. 이유와 한 줄 메모는 고르지 않아도 된다.
--
-- 세 표로 나눈다.
--   recommendation_batch     — 홈 추천 응답 한 번을 서버가 그대로 남긴 것. 화면이 보낸 코디를
--                              믿지 않고, 비교할 두 코디는 반드시 여기서 꺼낸다.
--   outfit_comparison        — 사용자에게 실제로 보여 준 두 코디(노출). 고르기 전에 쓴다.
--   outfit_comparison_vote   — 고른 결과. 비교 하나에 한 행(먼저 쓴 것이 남는다).
--
-- 기록만 한다. 운영 추천 점수나 학습에는 쓰지 않는다 — 한 사람의 그때 취향이지 정답이 아니다.
-- 테스트 fixture(`tests/fixtures/*pairs*`)와 섞지 않으려고 별도 표에 둔다.
-- 옷이 나중에 이름이 바뀌거나 지워져도 그때 무엇을 보여 줬는지 남아야 하므로, 아이템은
-- id 참조가 아니라 값으로 복사한다(`look_signal.outfit_json` 과 같은 이유).

CREATE TABLE IF NOT EXISTS recommendation_batch (
  id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  gender VARCHAR(10) DEFAULT NULL,
  mood_key VARCHAR(50) DEFAULT NULL,
  occasion VARCHAR(50) DEFAULT NULL,
  -- 서버가 실제로 쓴 날씨(기온·체감·설명). 모델이 쓴 날씨 문장이 아니다.
  weather_json JSON DEFAULT NULL,
  -- 응답에 실린 코디들: 모드·순서·아이템 스냅숏(id·이름·카테고리·색)·점수.
  candidates_json JSON NOT NULL,
  -- 후보 모델·프롬프트·엔진 버전과 그 값을 어디서 얻었는지. 모르는 값은 'unknown' 으로 둔다.
  provenance_json JSON NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_user_created (user_id, created_at)
);

CREATE TABLE IF NOT EXISTS outfit_comparison (
  id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  batch_id CHAR(36) NOT NULL,
  -- A 는 화면의 첫 번째(왼쪽·위) 코디다. 좌우는 만들 때 한 번만 무작위로 정하고 바꾸지 않는다.
  a_mode VARCHAR(30) NOT NULL,
  a_position TINYINT NOT NULL,
  -- 정렬한 아이템 id 를 '|' 로 이은 값. 같은 코디인지 가르는 기준이다.
  a_item_key VARCHAR(600) NOT NULL,
  a_items_json JSON NOT NULL,
  b_mode VARCHAR(30) NOT NULL,
  b_position TINYINT NOT NULL,
  b_item_key VARCHAR(600) NOT NULL,
  b_items_json JSON NOT NULL,
  -- A 가 응답 순서상 앞선 코디인지(1) 뒤의 코디를 앞에 놓았는지(0). 자리 효과를 따로 재려고 둔다.
  a_is_higher_ranked TINYINT(1) NOT NULL,
  -- 그 추천의 무드·성별·상황·날씨·버전 정보(`recommendation_batch` 에서 복사).
  context_json JSON NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  -- 추천 한 번에 비교 하나. 새로고침·두 번 누르기에도 같은 비교(같은 좌우)가 돌아온다.
  UNIQUE KEY uniq_user_batch (user_id, batch_id),
  INDEX idx_user_created (user_id, created_at),
  CONSTRAINT chk_comparison_distinct CHECK (a_item_key <> b_item_key)
);

CREATE TABLE IF NOT EXISTS outfit_comparison_vote (
  -- 비교 하나에 응답 하나. 중복 제출·동시 제출은 이 키가 막는다.
  comparison_id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  -- 'a' / 'b' 는 화면 자리 기준, 'neither' 는 둘 다 아쉬움. 건너뛰기는 행을 만들지 않는다.
  choice VARCHAR(10) NOT NULL,
  -- 고른 코디의 실제 정체(자리와 상관없이). 'neither' 면 비어 있다.
  chosen_mode VARCHAR(30) DEFAULT NULL,
  chosen_position TINYINT DEFAULT NULL,
  chosen_item_key VARCHAR(600) DEFAULT NULL,
  -- 이유의 방향: 'chosen'(고른 코디가 좋았던 점) / 'both_lacking'(둘 다 아쉬웠던 점).
  reason_polarity VARCHAR(20) NOT NULL,
  reasons_json JSON NOT NULL,
  comment VARCHAR(200) DEFAULT NULL,
  submitted_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_user_submitted (user_id, submitted_at),
  CONSTRAINT fk_vote_comparison FOREIGN KEY (comparison_id) REFERENCES outfit_comparison (id),
  CONSTRAINT chk_vote_choice CHECK (choice IN ('a', 'b', 'neither')),
  CONSTRAINT chk_vote_polarity CHECK (
    (choice = 'neither' AND reason_polarity = 'both_lacking' AND chosen_item_key IS NULL)
    OR (choice IN ('a', 'b') AND reason_polarity = 'chosen' AND chosen_item_key IS NOT NULL)
  )
);
