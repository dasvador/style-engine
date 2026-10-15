-- 스타일 상담의 대화를 남긴다.
--
-- 지금까지 상담은 화면 상태로만 있어서, 새로고침하거나 다른 화면에 다녀오면 대화와
-- 룩북 카드가 전부 사라졌다. 카드의 사진은 `outfit_image` 에 코디별로 남아 있으므로,
-- 여기에는 사진을 다시 찾는 데 필요한 것(아이템 목록과 무드)만 둔다. 같은 아이템·
-- 무드로 다시 요청하면 캐시에 걸려 비용 없이 같은 사진이 나온다.
--
-- `mood_key` 를 메시지마다 두는 이유: 사진 캐시 키에 무드가 들어간다. 화면의 현재 무드로
-- 그리면, 무드를 바꾼 뒤 예전 카드를 열 때마다 사진을 새로 만든다.

CREATE TABLE IF NOT EXISTS chat_message (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL,
  -- 'user' 또는 'ai'
  role VARCHAR(10) NOT NULL,
  text TEXT NOT NULL,
  -- AI 답에 코디가 있으면 그 아이템 목록. 없으면 말풍선이다.
  items_json JSON DEFAULT NULL,
  -- 바로 앞 카드의 아이템. 일부만 바꾼 코디는 앞 사진을 편집해 만들었다.
  base_items_json JSON DEFAULT NULL,
  mood_key VARCHAR(50) DEFAULT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_user_created (user_id, created_at)
);
