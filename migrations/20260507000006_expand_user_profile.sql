ALTER TABLE user_style_profile
  -- 체형 추가
  ADD COLUMN leg_length VARCHAR(20) DEFAULT NULL COMMENT 'short/average/long',
  ADD COLUMN preferred_top_size VARCHAR(10) DEFAULT NULL COMMENT 'S/M/L/XL/XXL',

  -- 취향: 선호
  ADD COLUMN likes_texture_depth BOOLEAN DEFAULT FALSE,
  ADD COLUMN likes_melange BOOLEAN DEFAULT FALSE,
  ADD COLUMN likes_suede BOOLEAN DEFAULT FALSE,
  ADD COLUMN likes_washed_denim BOOLEAN DEFAULT FALSE,
  ADD COLUMN likes_mocha_brown BOOLEAN DEFAULT FALSE,
  ADD COLUMN likes_heather_gray BOOLEAN DEFAULT FALSE,

  -- 취향: 비선호
  ADD COLUMN dislikes_flat_beige BOOLEAN DEFAULT FALSE,
  ADD COLUMN dislikes_military_cosplay BOOLEAN DEFAULT FALSE,
  ADD COLUMN dislikes_bright_colors BOOLEAN DEFAULT FALSE,

  -- 밸런스 룰
  ADD COLUMN low_profile_only_occasional BOOLEAN DEFAULT FALSE,
  ADD COLUMN medium_volume_runner_bonus BOOLEAN DEFAULT FALSE,
  ADD COLUMN denim_bridge_bonus BOOLEAN DEFAULT TRUE,

  -- 라이프스타일
  ADD COLUMN commute VARCHAR(30) DEFAULT NULL COMMENT 'public_transport/car/walk/bike',
  ADD COLUMN walking_amount VARCHAR(20) DEFAULT NULL COMMENT 'low/medium/high',
  ADD COLUMN comfort_priority VARCHAR(20) DEFAULT NULL COMMENT 'low/medium/high';

-- 기본 유저 프로파일 업데이트
UPDATE user_style_profile SET
  leg_length = 'short',
  preferred_top_size = 'XL',
  likes_texture_depth = TRUE,
  likes_melange = TRUE,
  likes_suede = TRUE,
  likes_washed_denim = TRUE,
  likes_mocha_brown = TRUE,
  likes_heather_gray = TRUE,
  dislikes_flat_beige = TRUE,
  dislikes_military_cosplay = TRUE,
  dislikes_bright_colors = TRUE,
  low_profile_only_occasional = TRUE,
  medium_volume_runner_bonus = TRUE,
  denim_bridge_bonus = TRUE,
  commute = 'public_transport',
  walking_amount = 'high',
  comfort_priority = 'medium_high'
WHERE user_id = 'default';
