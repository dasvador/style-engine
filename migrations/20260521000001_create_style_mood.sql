-- 성별별 스타일 무드 테이블
CREATE TABLE IF NOT EXISTS style_mood (
  id INT AUTO_INCREMENT PRIMARY KEY,
  gender ENUM('male', 'female', 'unisex') NOT NULL,
  mood_key VARCHAR(50) NOT NULL,
  mood_label VARCHAR(100) NOT NULL,
  description VARCHAR(255) DEFAULT NULL,
  sort_order INT DEFAULT 0,
  UNIQUE INDEX idx_gender_mood (gender, mood_key)
);

-- 초기 데이터
INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
-- 남성
('male', 'amekaji', '아메카지', '밀리터리/워크웨어/빈티지 믹스', 1),
('male', 'minimal', '미니멀 캐주얼', '깔끔하고 절제된 데일리', 2),
('male', 'street', '스트릿', '스트릿/힙합 캐주얼', 3),
-- 여성
('female', 'feminine_casual', '페미닌 캐주얼', '여성스러운 데일리 스타일', 1),
('female', 'boyish', '보이시 캐주얼', '보이프렌드핏/젠더리스', 2),
('female', 'minimal', '미니멀 시크', '깔끔하고 모던한 스타일', 3),
('female', 'street', '힙스터 스트릿', '힙스터/스트릿 캐주얼', 4),
-- 공용
('unisex', 'minimal', '미니멀', '젠더리스 미니멀 스타일', 1),
('unisex', 'street', '스트릿', '젠더리스 스트릿 스타일', 2);
