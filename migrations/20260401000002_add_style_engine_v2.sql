-- 스타일 엔진 v2: 추가 메타데이터

ALTER TABLE clothing
    ADD COLUMN color_temperature VARCHAR(20) NULL COMMENT 'warm/cool/neutral',
    ADD COLUMN versatility      VARCHAR(20) NULL COMMENT 'universal/flexible/situational/statement',
    ADD COLUMN statement_level   TINYINT     NULL COMMENT '1-5 존재감 레벨',
    ADD COLUMN formality_level   TINYINT     NULL COMMENT '1-5 포멀 레벨';

-- texture_world: 하나의 아이템이 여러 텍스처 월드에 속할 수 있음 (M:N)
CREATE TABLE IF NOT EXISTS clothing_texture_world (
    clothing_id CHAR(36) NOT NULL,
    texture_world VARCHAR(30) NOT NULL COMMENT 'workwear/military/tailoring/sweat/outdoor/minimal',
    PRIMARY KEY (clothing_id, texture_world),
    FOREIGN KEY (clothing_id) REFERENCES clothing(id) ON DELETE CASCADE
);
