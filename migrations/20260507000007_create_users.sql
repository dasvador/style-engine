CREATE TABLE IF NOT EXISTS app_user (
  id VARCHAR(36) PRIMARY KEY,
  username VARCHAR(50) NOT NULL UNIQUE,
  api_token VARCHAR(64) NOT NULL UNIQUE,
  display_name VARCHAR(100) DEFAULT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 기존 default 유저를 app_user로 마이그레이션
INSERT IGNORE INTO app_user (id, username, api_token, display_name)
VALUES ('default', 'default', 'dev-token-default', 'Default User');
