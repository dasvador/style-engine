ALTER TABLE clothing
  ADD COLUMN sub_category VARCHAR(30) DEFAULT NULL COMMENT 'tee/henley/shirt/knit/sweat/outer_shirt/jacket/coat/parka/denim/chino/cargo/slacks/sneaker/boots/loafer/backpack/tote/crossbody',
  ADD COLUMN floating_score TINYINT DEFAULT NULL COMMENT '1-10 (1=매우 접지, 10=매우 떠보임)',
  ADD COLUMN strong_style_score TINYINT DEFAULT NULL COMMENT '1-10 (1=매우 중립, 10=매우 강한 스타일)',
  ADD COLUMN texture_keywords VARCHAR(200) DEFAULT NULL COMMENT 'washed,faded,slubby,melange,brushed,glossy,dry,hairy 등 쉼표 구분';
