-- 아이템 물성 컬럼 (1-10 스케일)
ALTER TABLE clothing
  ADD COLUMN visual_weight_v2 TINYINT DEFAULT NULL COMMENT '시각적 무게 1-10',
  ADD COLUMN texture_depth_v2 TINYINT DEFAULT NULL COMMENT '질감 깊이 1-10',
  ADD COLUMN grounding_score TINYINT DEFAULT NULL COMMENT '접지감 1-10 (신발/가방)',
  ADD COLUMN shadow_tone VARCHAR(20) DEFAULT NULL COMMENT 'clean/washed/faded/dusty',
  ADD COLUMN silhouette_volume VARCHAR(20) DEFAULT NULL COMMENT 'slim/regular/relaxed/oversized',
  ADD COLUMN material_primary VARCHAR(30) DEFAULT NULL COMMENT 'jersey/cotton/linen/denim/suede/leather/nylon/wool/knit/canvas/corduroy/flannel';

-- 유저 스타일 프로파일
CREATE TABLE IF NOT EXISTS user_style_profile (
  user_id VARCHAR(50) PRIMARY KEY,
  height_cm INT DEFAULT NULL,
  weight_kg INT DEFAULT NULL,
  upper_body VARCHAR(20) DEFAULT NULL COMMENT 'small/medium/large',
  calves VARCHAR(20) DEFAULT NULL COMMENT 'thin/medium/thick',
  preferred_fit VARCHAR(20) DEFAULT NULL COMMENT 'slim/regular/relaxed/oversized',
  needs_grounded_shoes BOOLEAN DEFAULT FALSE,
  prefers_weighted_bag BOOLEAN DEFAULT FALSE,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

-- 기본 유저 프로파일 삽입
INSERT IGNORE INTO user_style_profile (user_id, height_cm, weight_kg, upper_body, calves, preferred_fit, needs_grounded_shoes, prefers_weighted_bag)
VALUES ('default', 174, 87, 'large', 'thick', 'relaxed', TRUE, TRUE);
