-- `pop.sql` 을 되돌린다. 시드가 넣은 것만 지운다 — 기존 옷의 원래 태그와 값은 처음부터
-- 건드리지 않았으므로 그대로다.
--
-- 태그는 이 시드가 붙인 옷으로만 좁힌다: 새 옷 범위(`0ffd0f00-…`)와 시드가 팝 태그를 더한 기존 옷
-- 13벌(`pop.sql` 의 목록과 같다 — `tests/style_genre_migration.rs` 가 대조한다). 다른 시드가 다른
-- 옷에 붙인 팝 태그와 사용자·분석이 붙인 팝 태그는 남는다.
--
-- 새 옷이 룩북·추천 이력에 이미 쓰였다면 그 기록(이름·JSON)은 남는다. 옷 행만 사라진다.
DELETE FROM clothing_style_genre
WHERE style_genre = 'pop' AND source = 'seed'
  AND (clothing_id LIKE '0ffd0f00-0000-0000-0000-%'
       OR clothing_id IN (
  '0ffd0700-0000-0000-0000-000000000000',
  'd8929e84-546b-11f1-af43-264d116ef584',
  '0ffd0800-0000-0000-0000-000000000007',
  '8c004cbc-598c-11f1-af43-264d116ef584',
  'd892aeb0-546b-11f1-af43-264d116ef584',
  '0ffd0700-0000-0000-0000-000000000007',
  '0ffd0700-0000-0000-0000-00000000000a',
  'd892c5d0-546b-11f1-af43-264d116ef584',
  '0ffd0700-0000-0000-0000-00000000000c',
  '0ffd0b00-0000-0000-0000-000000000009',
  '0ffd0c00-0000-0000-0000-000000000010',
  '0ffd0800-0000-0000-0000-000000000000',
  '8c0052ca-598c-11f1-af43-264d116ef584'
       ));
-- 새 옷은 이 시드가 만든 행이라 남은 태그(다른 장르 포함)·계절·텍스처까지 함께 지운다.
DELETE FROM clothing_style_genre WHERE clothing_id LIKE '0ffd0f00-0000-0000-0000-%';
DELETE FROM clothing_texture_world WHERE clothing_id LIKE '0ffd0f00-0000-0000-0000-%';
DELETE FROM clothing_season WHERE clothing_id LIKE '0ffd0f00-0000-0000-0000-%';
DELETE FROM clothing WHERE id LIKE '0ffd0f00-0000-0000-0000-%';
DELETE FROM style_mood WHERE gender = 'female' AND mood_key = 'pop';
