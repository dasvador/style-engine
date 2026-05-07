CREATE TABLE IF NOT EXISTS outfit_image (
  id VARCHAR(36) PRIMARY KEY,
  user_id VARCHAR(50) NOT NULL DEFAULT 'default',
  outfit_hash VARCHAR(64) NOT NULL COMMENT 'items 이름 정렬 해시',
  prompt_hash VARCHAR(64) NOT NULL COMMENT 'prompt 해시',
  image_path VARCHAR(255) NOT NULL,
  prompt_text TEXT DEFAULT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE INDEX idx_outfit_prompt (outfit_hash, prompt_hash),
  INDEX idx_user (user_id)
);
