-- 옷장 아이템에 소유자를 붙인다(계정별 옷장 격리).
--
-- 기존 아이템은 모두 'default' 사용자 것으로 남는다. 다른 계정에 자동으로 배정하지 않고 지우지도 않는다 —
-- 기존 옷장을 다른 계정에 넘기려면 소유자가 명시적으로 옮겨야 한다(README "기존 default 옷장").
ALTER TABLE clothing ADD COLUMN user_id VARCHAR(50) NOT NULL DEFAULT 'default';
CREATE INDEX idx_clothing_user ON clothing (user_id, created_at);

-- 20260507000007 이 default 사용자의 토큰을 저장소에 평문('dev-token-default')으로 남겼다. 이 값을 알면
-- 누구나 기존 옷장에 접근할 수 있으므로 무효화한다(행·데이터는 그대로). 로컬 화면은 토큰 없이
-- 명시적 개발 모드(WARDROBE_AUTH_MODE=dev-default-loopback)로만 default 를 쓴다.
UPDATE app_user
   SET api_token = CONCAT('revoked-', REPLACE(UUID(), '-', ''))
 WHERE id = 'default' AND api_token = 'dev-token-default';
