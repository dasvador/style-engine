-- reason 태그 테이블 (outfit_feedback과 1:N)
CREATE TABLE IF NOT EXISTS feedback_reason (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  feedback_id VARCHAR(36) NOT NULL,
  user_id VARCHAR(50) NOT NULL DEFAULT 'default',
  reason_tag VARCHAR(50) NOT NULL COMMENT 'too_military/good_texture/etc',
  polarity VARCHAR(10) NOT NULL COMMENT 'positive/negative',
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_user_tag (user_id, reason_tag),
  INDEX idx_feedback (feedback_id)
);

-- 유저 선호 프로파일 (reason tag 누적 → scoring 가중치)
CREATE TABLE IF NOT EXISTS user_preference_score (
  user_id VARCHAR(50) NOT NULL DEFAULT 'default',
  reason_tag VARCHAR(50) NOT NULL,
  score INT NOT NULL DEFAULT 0 COMMENT '양수=선호, 음수=비선호',
  count INT NOT NULL DEFAULT 0,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (user_id, reason_tag)
);

-- item_feedback_score의 delta를 줄임 (item -1, pattern -3으로 재조정)
-- 기존 데이터는 유지, 로직에서 가중치 조정
