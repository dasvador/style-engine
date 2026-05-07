-- 착장 추천 피드백
CREATE TABLE IF NOT EXISTS outfit_feedback (
  id VARCHAR(36) PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL DEFAULT 'default',
  feedback_type VARCHAR(20) NOT NULL COMMENT 'like/dislike/worn/saved/skipped',
  reason VARCHAR(100) DEFAULT NULL COMMENT 'too_military/floating/good_texture/etc',
  -- 착장 아이템 (이름 기반)
  inner_name VARCHAR(100) DEFAULT NULL,
  outer_name VARCHAR(100) DEFAULT NULL,
  bottom_name VARCHAR(100) DEFAULT NULL,
  shoes_name VARCHAR(100) DEFAULT NULL,
  bag_name VARCHAR(100) DEFAULT NULL,
  -- anchor (유저가 지정한 아이템)
  anchor_name VARCHAR(100) DEFAULT NULL,
  -- 자유 텍스트 피드백
  comment TEXT DEFAULT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,

  INDEX idx_user_type (user_id, feedback_type),
  INDEX idx_created (created_at)
);

-- 피드백 기반 아이템별 보정 점수
CREATE TABLE IF NOT EXISTS item_feedback_score (
  user_id VARCHAR(50) NOT NULL DEFAULT 'default',
  item_name VARCHAR(100) NOT NULL,
  score_adjustment INT NOT NULL DEFAULT 0 COMMENT '누적 보정 (+는 선호, -는 비선호)',
  feedback_count INT NOT NULL DEFAULT 0,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (user_id, item_name)
);
