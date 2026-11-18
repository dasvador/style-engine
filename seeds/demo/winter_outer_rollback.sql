-- `winter_outer.sql` 을 되돌린다. 시드의 31개 id 중 user_id = 'default' 인 옷과 그 옷의 계절·
-- 텍스처 월드·장르 태그만 지운다. 같은 id 를 다른 사용자가 갖고 있으면 그 행과 자식 행은 남는다.
--
-- 차콜 울 오버셔츠 이름 복원은 되돌리지 않는다 — 깨진 글자로 되돌릴 이유가 없다. 새 옷이 추천
-- 이력·룩북·착장 기록에 이미 쓰였다면 그 기록은 이 파일이 지우지 않는다. 한 트랜잭션, 재실행 안전.

START TRANSACTION;

DROP TEMPORARY TABLE IF EXISTS winter_outer_ids;
CREATE TEMPORARY TABLE winter_outer_ids AS SELECT id FROM clothing WHERE 1 = 0;
INSERT INTO winter_outer_ids (id) VALUES
  ('0ffd1100-0000-0000-0000-000000000101'),
  ('0ffd1100-0000-0000-0000-000000000102'),
  ('0ffd1100-0000-0000-0000-000000000103'),
  ('0ffd1100-0000-0000-0000-000000000104'),
  ('0ffd1100-0000-0000-0000-000000000105'),
  ('0ffd1100-0000-0000-0000-000000000106'),
  ('0ffd1100-0000-0000-0000-000000000107'),
  ('0ffd1100-0000-0000-0000-000000000108'),
  ('0ffd1100-0000-0000-0000-000000000109'),
  ('0ffd1100-0000-0000-0000-000000000110'),
  ('0ffd1100-0000-0000-0000-000000000111'),
  ('0ffd1100-0000-0000-0000-000000000112'),
  ('0ffd1100-0000-0000-0000-000000000113'),
  ('0ffd1100-0000-0000-0000-000000000114'),
  ('0ffd1100-0000-0000-0000-000000000115'),
  ('0ffd1100-0000-0000-0000-000000000116'),
  ('0ffd1100-0000-0000-0000-000000000201'),
  ('0ffd1100-0000-0000-0000-000000000202'),
  ('0ffd1100-0000-0000-0000-000000000203'),
  ('0ffd1100-0000-0000-0000-000000000204'),
  ('0ffd1100-0000-0000-0000-000000000205'),
  ('0ffd1100-0000-0000-0000-000000000206'),
  ('0ffd1100-0000-0000-0000-000000000207'),
  ('0ffd1100-0000-0000-0000-000000000208'),
  ('0ffd1100-0000-0000-0000-000000000209'),
  ('0ffd1100-0000-0000-0000-000000000210'),
  ('0ffd1100-0000-0000-0000-000000000211'),
  ('0ffd1100-0000-0000-0000-000000000212'),
  ('0ffd1100-0000-0000-0000-000000000213'),
  ('0ffd1100-0000-0000-0000-000000000214'),
  ('0ffd1100-0000-0000-0000-000000000215');

DELETE g FROM clothing_style_genre g
  JOIN clothing c ON c.id = g.clothing_id AND c.user_id = 'default'
  JOIN winter_outer_ids t ON t.id = c.id;
DELETE w FROM clothing_texture_world w
  JOIN clothing c ON c.id = w.clothing_id AND c.user_id = 'default'
  JOIN winter_outer_ids t ON t.id = c.id;
DELETE s FROM clothing_season s
  JOIN clothing c ON c.id = s.clothing_id AND c.user_id = 'default'
  JOIN winter_outer_ids t ON t.id = c.id;
DELETE c FROM clothing c
  JOIN winter_outer_ids t ON t.id = c.id
 WHERE c.user_id = 'default';

DROP TEMPORARY TABLE winter_outer_ids;

COMMIT;
