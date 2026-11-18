-- `edgy_bottom_colour.sql` 을 되돌린다. 네 id 중 user_id = 'default' 인 옷과 그 옷의 계절·
-- 텍스처 월드·장르 태그만 지운다. 추천 이력·룩북 기록은 지우지 않는다. 한 트랜잭션, 재실행 안전.

START TRANSACTION;

DROP TEMPORARY TABLE IF EXISTS edgy_bottom_colour_ids;
CREATE TEMPORARY TABLE edgy_bottom_colour_ids AS SELECT id FROM clothing WHERE 1 = 0;
INSERT INTO edgy_bottom_colour_ids (id) VALUES
  ('0ffd1200-0000-0000-0000-000000000301'),
  ('0ffd1200-0000-0000-0000-000000000302'),
  ('0ffd1200-0000-0000-0000-000000000303'),
  ('0ffd1200-0000-0000-0000-000000000304');

DELETE g FROM clothing_style_genre g
  JOIN clothing c ON c.id = g.clothing_id AND c.user_id = 'default'
  JOIN edgy_bottom_colour_ids t ON t.id = c.id;
DELETE w FROM clothing_texture_world w
  JOIN clothing c ON c.id = w.clothing_id AND c.user_id = 'default'
  JOIN edgy_bottom_colour_ids t ON t.id = c.id;
DELETE s FROM clothing_season s
  JOIN clothing c ON c.id = s.clothing_id AND c.user_id = 'default'
  JOIN edgy_bottom_colour_ids t ON t.id = c.id;
DELETE c FROM clothing c
  JOIN edgy_bottom_colour_ids t ON t.id = c.id
 WHERE c.user_id = 'default';

DROP TEMPORARY TABLE edgy_bottom_colour_ids;

COMMIT;
