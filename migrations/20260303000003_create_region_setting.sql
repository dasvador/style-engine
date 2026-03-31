CREATE TABLE IF NOT EXISTS region_setting (
    id         CHAR(36)     PRIMARY KEY,
    name       VARCHAR(100) NOT NULL,
    latitude   DOUBLE       NOT NULL,
    longitude  DOUBLE       NOT NULL,
    updated_at DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

-- Insert a default region (Seoul)
INSERT IGNORE INTO region_setting (id, name, latitude, longitude)
VALUES (UUID(), '서울', 37.5665, 126.9780);
