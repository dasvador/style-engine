-- `glam.sql` 을 되돌린다. 시드가 넣은 것만 지운다 — 기존 옷의 원래 태그와 값은
-- 처음부터 건드리지 않았으므로 그대로다.
--
-- 새 옷이 룩북·추천 이력에 이미 쓰였다면 그 기록(이름·JSON)은 남는다. 옷 행만 사라진다.
DELETE FROM clothing_style_genre WHERE style_genre = 'glam' AND source = 'seed';
DELETE FROM clothing_texture_world WHERE clothing_id LIKE '0ffd0b00-0000-0000-0000-%';
DELETE FROM clothing_season WHERE clothing_id LIKE '0ffd0b00-0000-0000-0000-%';
DELETE FROM clothing WHERE id LIKE '0ffd0b00-0000-0000-0000-%';
DELETE FROM style_mood WHERE gender = 'female' AND mood_key = 'glam';
