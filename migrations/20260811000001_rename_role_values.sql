-- Rename the outfit role vocabulary away from the rice/side-dish metaphor.
--
-- The engine already reasons about these values in English (TooManyAccents,
-- LackOfStructure), so the Korean labels are aligned with that: an item is
-- either the neutral base of an outfit or an accent placed on top of it.
--
--   밥       -> 베이스     (base)
--   반찬     -> 포인트     (accent)
--   약한반찬 -> 약한포인트  (soft accent)
--
-- 연결템 / 구조템 are already neutral and stay as they are.

UPDATE clothing SET role = '베이스'     WHERE role = '밥';
UPDATE clothing SET role = '약한포인트' WHERE role = '약한반찬';
UPDATE clothing SET role = '포인트'     WHERE role = '반찬';

ALTER TABLE clothing
    MODIFY COLUMN role VARCHAR(20) NULL
    COMMENT '베이스/포인트/약한포인트/연결템/구조템';
