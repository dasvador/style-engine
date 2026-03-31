CREATE TABLE IF NOT EXISTS clothing_season (
    clothing_id CHAR(36)    NOT NULL,
    season      VARCHAR(20) NOT NULL,
    PRIMARY KEY (clothing_id, season),
    FOREIGN KEY (clothing_id) REFERENCES clothing(id) ON DELETE CASCADE
);
