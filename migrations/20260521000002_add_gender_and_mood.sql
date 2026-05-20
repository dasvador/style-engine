-- user_style_profile에 gender 추가
ALTER TABLE user_style_profile
  ADD COLUMN gender ENUM('male', 'female') DEFAULT 'male' AFTER user_id;

-- clothing에 gender + style_mood 추가
ALTER TABLE clothing
  ADD COLUMN gender ENUM('male', 'female', 'unisex') DEFAULT 'male' AFTER category,
  ADD COLUMN style_mood VARCHAR(50) DEFAULT 'amekaji' AFTER gender;

-- 기존 아이템 태깅 (전부 남성 아메카지)
UPDATE clothing SET gender = 'male', style_mood = 'amekaji' WHERE gender IS NULL OR gender = 'male';
