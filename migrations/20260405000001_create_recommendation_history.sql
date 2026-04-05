CREATE TABLE IF NOT EXISTS outfit_recommendation_history (
    id CHAR(36) PRIMARY KEY,
    user_id CHAR(36) NOT NULL,
    top_id CHAR(36) NULL,
    bottom_id CHAR(36) NULL,
    outer_id CHAR(36) NULL,
    shoes_id CHAR(36) NULL,
    bag_id CHAR(36) NULL,
    recommended_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    INDEX idx_outfit_history_user_date (user_id, recommended_at),
    INDEX idx_outfit_history_top (user_id, top_id, recommended_at),
    INDEX idx_outfit_history_bottom (user_id, bottom_id, recommended_at),
    INDEX idx_outfit_history_outer (user_id, outer_id, recommended_at),
    INDEX idx_outfit_history_shoes (user_id, shoes_id, recommended_at),
    INDEX idx_outfit_history_bag (user_id, bag_id, recommended_at)
);
