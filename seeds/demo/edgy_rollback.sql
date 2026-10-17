-- `edgy.sql` 을 되돌린다. 시드가 넣은 것만 지운다 — 기존 옷의 원래 태그와 값은 처음부터
-- 건드리지 않았으므로 그대로다.
--
-- 태그는 이 시드가 붙인 옷으로만 좁힌다: 새 옷 범위(`0ffd0c00-…`)와 시드가 엣지 태그를
-- 더한 기존 옷 17벌(`edgy.sql` 의 목록과 같다 — `tests/style_genre_migration.rs` 가 대조한다).
-- 다른 시드가 다른 옷에 붙인 엣지 태그(`source = 'seed'`)와 사용자·분석이 붙인 엣지 태그
-- (`source` 가 'user'·'vision')는 남는다. 기존 옷에 이미 사용자·분석의 엣지 태그가 있었다면
-- 시드의 INSERT IGNORE 가 그 행을 바꾸지 않았으므로 여기서도 지워지지 않는다.
--
-- 새 옷이 룩북·추천 이력에 이미 쓰였다면 그 기록(이름·JSON)은 남는다. 옷 행만 사라진다.
DELETE FROM clothing_style_genre
WHERE style_genre = 'edgy' AND source = 'seed'
  AND (clothing_id LIKE '0ffd0c00-0000-0000-0000-%'
       OR clothing_id IN (
  '0ffd0700-0000-0000-0000-000000000008',
  'd892bf4a-546b-11f1-af43-264d116ef584',
  '0ffd0a00-0000-0000-0000-000000000000',
  '0ffd0b00-0000-0000-0000-000000000012',
  'd8929e84-546b-11f1-af43-264d116ef584',
  '8e2f79ba-598a-11f1-af43-264d116ef584',
  '0ffd0700-0000-0000-0000-000000000003',
  '0ffd0700-0000-0000-0000-000000000000',
  '0ffd0700-0000-0000-0000-000000000006',
  '0ffd0800-0000-0000-0000-000000000007',
  'd892aeb0-546b-11f1-af43-264d116ef584',
  '0ffd0a00-0000-0000-0000-000000000002',
  '0ffd0700-0000-0000-0000-00000000000c',
  '0ffd0800-0000-0000-0000-000000000003',
  '0ffd0b00-0000-0000-0000-000000000009',
  '8e2f841e-598a-11f1-af43-264d116ef584',
  'd892b450-546b-11f1-af43-264d116ef584'
       ));
-- 새 옷은 이 시드가 만든 행이라 남은 태그(다른 장르 포함)·계절·텍스처까지 함께 지운다.
DELETE FROM clothing_style_genre WHERE clothing_id LIKE '0ffd0c00-0000-0000-0000-%';
DELETE FROM clothing_texture_world WHERE clothing_id LIKE '0ffd0c00-0000-0000-0000-%';
DELETE FROM clothing_season WHERE clothing_id LIKE '0ffd0c00-0000-0000-0000-%';
DELETE FROM clothing WHERE id LIKE '0ffd0c00-0000-0000-0000-%';
DELETE FROM style_mood WHERE gender = 'female' AND mood_key = 'edgy';
