-- visual_weight: 아이템의 시각적 무게감 (light / medium / heavy)
-- texture_depth: 소재 질감 깊이 (flat / medium / rich)
ALTER TABLE clothing
  ADD COLUMN visual_weight VARCHAR(20) DEFAULT NULL,
  ADD COLUMN texture_depth VARCHAR(20) DEFAULT NULL;
