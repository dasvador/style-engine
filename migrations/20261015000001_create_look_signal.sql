-- 홈 추천에서 보여 준 코디와 그 뒤의 반응을 요청 단위로 남긴다.
--
-- 엔진이 사람 취향과 맞는지 재려면 "같은 조건에서 A보다 B" 쌍이 필요하다. 지금까지는
-- 사람이 검수 화면에서 일일이 골라야 했다. 그런데 홈은 한 번 추천할 때 나온 코디를
-- 전부 룩북에 저장하므로, 같은 요청 안에서 좋아요·입어봄을 받은 코디와 못 받은
-- 코디를 비교하면 쌍이 저절로 생긴다.
--
-- `lookbook_look` 에 붙이지 않는 이유: 룩북은 같은 코디를 한 행으로 합친다
-- (`uniq_user_look`). 같은 코디가 두 요청에 나오면 어느 요청이었는지 잃는다.
-- 또 룩북에서 지우면 행이 사라져, 지웠다는 사실(가장 분명한 싫음)이 남지 않는다.
-- 여기는 요청마다 한 행씩 쌓고 지우지 않는다.
--
-- 이 표는 기록만 한다. 운영 추천 점수에는 쓰지 않는다 — 쌍이 모이면 섀도로 먼저 잰다.

CREATE TABLE IF NOT EXISTS look_signal (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  -- 화면이 추천 한 번마다 만드는 값. 같은 값끼리가 한 비교 묶음이다.
  request_id CHAR(36) NOT NULL,
  -- 룩북 행 id. 룩북에서 지워져도 이 행은 남으므로 외래 키를 걸지 않는다.
  look_id CHAR(36) NOT NULL,
  -- 응답 안의 순서(0부터). 엔진이 어느 코디를 앞에 두었는지.
  position TINYINT NOT NULL DEFAULT 0,
  gender VARCHAR(10) DEFAULT NULL,
  mood_key VARCHAR(50) DEFAULT NULL,
  weather_summary VARCHAR(255) DEFAULT NULL,
  -- 그때 보여 준 아이템 목록. 옷이 나중에 지워져도 남아야 하므로 값으로 복사한다.
  outfit_json JSON NOT NULL,
  liked TINYINT(1) NOT NULL DEFAULT 0,
  worn TINYINT(1) NOT NULL DEFAULT 0,
  deleted TINYINT(1) NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uniq_request_look (request_id, look_id),
  INDEX idx_user_request (user_id, request_id),
  INDEX idx_look (look_id)
);

-- 쌍을 바로 볼 수 있게 묶어 둔다. 같은 요청 안에서
--   나은 쪽: 좋아요 또는 입어봄을 받고 지우지 않은 코디
--   못한 쪽: 지운 코디(strong) 또는 아무 반응도 없는 코디(weak)
-- weak 는 "못 봤다"와 "별로였다"를 가르지 못하므로 따로 센다.
CREATE OR REPLACE VIEW look_pair AS
SELECT b.user_id, b.request_id, b.gender, b.mood_key, b.weather_summary,
       b.look_id AS better_look_id, b.outfit_json AS better_outfit, b.position AS better_position,
       w.look_id AS worse_look_id, w.outfit_json AS worse_outfit, w.position AS worse_position,
       IF(w.deleted = 1, 'strong', 'weak') AS strength,
       b.created_at
FROM look_signal b
JOIN look_signal w
  ON w.request_id = b.request_id AND w.user_id = b.user_id AND w.look_id <> b.look_id
WHERE (b.liked = 1 OR b.worn = 1) AND b.deleted = 0
  AND (w.deleted = 1 OR (w.liked = 0 AND w.worn = 0));
