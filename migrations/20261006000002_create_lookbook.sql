-- 만들어진 코디를 남긴다.
--
-- 이미지 자체는 `outfit_image` 에 파일로 남지만, "어떤 코디였는지" 는 어디에도
-- 없었다. 화면 상태로만 들고 있어서 새로고침하면 사라졌다. 한 장에 약 1.2센트를
-- 들여 만든 결과가 매번 날아가는 셈이다.
--
-- `image_prompt` 를 그대로 보관하는 이유: 이미지 캐시는 이 문자열의 해시로 걸린다
-- (`routes::chat::generate_image` 의 `prompt_hash`). 아이템 이름이나 id 만 저장해
-- 두고 나중에 문자열을 다시 조립하면, 조립 규칙이 조금만 달라져도 해시가 어긋나
-- 캐시를 빗나가고 같은 그림을 돈 주고 다시 만든다. 쓰던 문자열을 그대로 두면
-- 다시 열 때 반드시 캐시에 걸린다.
--
-- `outfit_json` 은 화면에 뿌릴 아이템 목록이다. 옷이 나중에 삭제돼도 그때 무엇을
-- 입었는지는 남아야 하므로 id 참조가 아니라 값으로 복사해 둔다.

CREATE TABLE IF NOT EXISTS lookbook_look (
  id CHAR(36) NOT NULL PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  mood_key VARCHAR(50) DEFAULT NULL,
  title VARCHAR(100) NOT NULL,
  weather_summary VARCHAR(255) DEFAULT NULL,
  reason TEXT,
  recommendation TEXT,
  -- 이미지 캐시 키. 같은 사용자가 같은 코디를 다시 만들면 한 행만 남는다.
  image_prompt VARCHAR(600) NOT NULL,
  outfit_json JSON NOT NULL,
  liked TINYINT(1) NOT NULL DEFAULT 0,
  worn TINYINT(1) NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uniq_user_look (user_id, image_prompt),
  INDEX idx_user_created (user_id, created_at)
);
