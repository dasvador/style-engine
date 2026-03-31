ALTER TABLE clothing
    ADD COLUMN tone       VARCHAR(20) NULL COMMENT '밝음/중간/어두움',
    ADD COLUMN saturation VARCHAR(20) NULL COMMENT '낮음/중간/높음',
    ADD COLUMN style      VARCHAR(50) NULL COMMENT '베이직/워크/밀리터리/포멀/스포츠',
    ADD COLUMN weight     VARCHAR(20) NULL COMMENT '가벼움/중간/무거움',
    ADD COLUMN role       VARCHAR(20) NULL COMMENT '밥/반찬/약한반찬/연결템';
