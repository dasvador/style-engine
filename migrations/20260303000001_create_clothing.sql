CREATE TABLE IF NOT EXISTS clothing (
    id          CHAR(36)     PRIMARY KEY,
    name        VARCHAR(100) NOT NULL,
    category    VARCHAR(50)  NOT NULL,
    color       VARCHAR(50),
    thickness   VARCHAR(20)  NOT NULL DEFAULT 'medium',
    image_url   TEXT,
    created_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);
