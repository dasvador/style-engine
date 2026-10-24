-- 지역(날씨 기준 위치) 설정을 계정별로 나눈다.
--
-- 지금까지는 전역 한 행이었고, 저장할 때 모든 행을 지웠다 — 한 계정이 바꾸면 모두의 위치가 바뀐다.
-- 기존 행은 'default' 계정 것으로 남긴다(지우거나 다른 계정에 복사하지 않는다). 새 계정은 자기 지역을
-- 정하기 전까지 지역이 없다(다른 계정 위치를 대신 쓰지 않는다).
ALTER TABLE region_setting ADD COLUMN user_id VARCHAR(50) NOT NULL DEFAULT 'default';
CREATE INDEX idx_region_user ON region_setting (user_id, updated_at);
