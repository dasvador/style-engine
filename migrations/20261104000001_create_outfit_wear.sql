-- 매일 쓰는 코디 흐름: 추천 → 한 벌 바꾸기 → 입을 계획 → 실제 착용 기록.
--
-- 두 표로 나눈다.
--   outfit_ref   — 서버가 만든 코디 참조. 추천 응답·바꾸기 제안·룩북 코디를 서버가 검증한 값으로 남긴다.
--                  후속 요청은 이 id 만 보낸다. 모델이나 화면이 적어 보낸 옷 이름·id 는 저장 근거가 아니다.
--                  '2번 코디' 같은 말은 대화가 바뀌면 뜻이 없어서 정확한 참조가 있어야 한다.
--   outfit_wear  — 계획·착용·취소 기록. 날짜(서울 기준)와 그때의 스냅숏을 값으로 복사한다.
--
-- 기존 `lookbook_look.worn`, `outfit_feedback` 의 'worn' 은 날짜 없는 "입어 볼래요" 표시다. 실제 착용이라는
-- 근거가 없으므로 여기로 옮기지 않는다.
--
-- 추천 이력(`outfit_recommendation_history`)은 "보여 줬다" 이고, 여기의 status='worn' 만 "입었다" 다.
--
-- IF NOT EXISTS: 표를 만드는 문장뿐이라 다시 돌려도 기존 행을 건드리지 않는다.

CREATE TABLE IF NOT EXISTS outfit_ref (
  id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  -- 'recommendation' · 'replacement' · 'lookbook'
  source VARCHAR(20) NOT NULL,
  -- 같은 결과를 다시 만들어도 참조가 늘지 않게 하는 키(추천 묶음+순서, 원본 참조+바꾼 옷, 룩 id).
  dedupe_key VARCHAR(160) NOT NULL,
  batch_id CHAR(36) DEFAULT NULL,
  mode VARCHAR(30) DEFAULT NULL,
  -- 바꾸기 제안이면 원본 참조와 바꾼 카테고리.
  parent_ref_id CHAR(36) DEFAULT NULL,
  replaced_category VARCHAR(10) DEFAULT NULL,
  -- 정렬한 아이템 id 를 '|' 로 이은 값(`outfit_comparison::item_key`).
  item_key VARCHAR(600) NOT NULL,
  -- id·이름·카테고리·색 스냅숏. 옷이 바뀌거나 지워져도 그때의 값을 남긴다.
  items_json JSON NOT NULL,
  -- 상황·무드·성별·날씨 등 서버가 실제로 쓴 값.
  context_json JSON NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uniq_ref_dedupe (user_id, dedupe_key),
  INDEX idx_ref_user_created (user_id, created_at),
  CONSTRAINT chk_ref_source CHECK (source IN ('recommendation', 'replacement', 'lookbook'))
);

CREATE TABLE IF NOT EXISTS outfit_wear (
  id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  outfit_ref_id CHAR(36) NOT NULL,
  -- 'planned'(입을 계획) · 'worn'(실제로 입음, 사용자가 직접 말했을 때만) · 'cancelled'
  status VARCHAR(10) NOT NULL,
  -- 서울 기준 날짜.
  wear_date DATE NOT NULL,
  item_key VARCHAR(600) NOT NULL,
  items_json JSON NOT NULL,
  context_json JSON NOT NULL,
  -- 입어 본 뒤의 한 단어 반응. 기록일 뿐 추천 점수에 자동으로 쓰지 않는다.
  reaction VARCHAR(20) DEFAULT NULL,
  comment VARCHAR(200) DEFAULT NULL,
  -- 수정·취소의 충돌 방지. 상태나 내용이 바뀔 때마다 1 오른다.
  revision INT NOT NULL DEFAULT 1,
  planned_at DATETIME DEFAULT NULL,
  worn_at DATETIME DEFAULT NULL,
  cancelled_at DATETIME DEFAULT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  item_key_hash CHAR(64) GENERATED ALWAYS AS (SHA2(item_key, 256)) STORED,
  -- 취소된 행은 NULL 이라 유일 키에 걸리지 않는다. 취소 뒤 같은 코디를 다시 계획할 수 있다.
  active TINYINT GENERATED ALWAYS AS (IF(status = 'cancelled', NULL, 1)) STORED,
  -- 같은 날 같은 구성은 살아 있는 행 하나뿐이다(재시도·두 번 누르기 방지).
  -- 구성이 다른 코디는 하루에 몇 벌이든 된다 — 낮·저녁을 갈아입는 날이 있다.
  UNIQUE KEY uniq_active_outfit_day (user_id, wear_date, item_key_hash, active),
  INDEX idx_wear_user_date (user_id, wear_date),
  INDEX idx_wear_user_status_date (user_id, status, wear_date),
  CONSTRAINT fk_wear_ref FOREIGN KEY (outfit_ref_id) REFERENCES outfit_ref (id),
  CONSTRAINT chk_wear_status CHECK (status IN ('planned', 'worn', 'cancelled')),
  CONSTRAINT chk_wear_reaction CHECK (reaction IS NULL OR status = 'worn')
);
