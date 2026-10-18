-- `editorial.sql` 을 되돌린다. 시드가 넣은 것만 지운다.
--
-- 기존 옷의 태그는 이 시드가 붙인 한 줄(워시드 미디엄블루 와이드 데님의 로맨틱 seed 태그)만 지운다 —
-- 같은 옷의 원래 태그, 다른 옷의 로맨틱 태그, 사용자·분석이 붙인 태그는 남는다. 새 옷이 룩북·추천
-- 이력에 이미 쓰였다면 그 기록(이름·JSON)은 남는다. 옷 행만 사라진다.
DELETE FROM clothing_style_genre
WHERE style_genre = 'romantic' AND source = 'seed'
  AND clothing_id = 'd89291be-546b-11f1-af43-264d116ef584';
DELETE FROM clothing_style_genre WHERE clothing_id LIKE '0ffd1000-0000-0000-0000-%';
DELETE FROM clothing_texture_world WHERE clothing_id LIKE '0ffd1000-0000-0000-0000-%';
DELETE FROM clothing_season WHERE clothing_id LIKE '0ffd1000-0000-0000-0000-%';
DELETE FROM clothing WHERE id LIKE '0ffd1000-0000-0000-0000-%';
