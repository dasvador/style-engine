-- `street_sporty_color.sql` 을 되돌린다. 이 시드가 만든 옷 12벌(`0ffd0e00-…`)과 그 관계만
-- 지운다. 기존 옷·태그·무드 칩, 사용자·분석이 붙인 태그는 그대로다. 새 옷이 룩북·추천
-- 이력에 이미 쓰였다면 그 기록(이름·JSON)은 남는다.
DELETE FROM clothing_style_genre WHERE clothing_id LIKE '0ffd0e00-0000-0000-0000-%';
DELETE FROM clothing_texture_world WHERE clothing_id LIKE '0ffd0e00-0000-0000-0000-%';
DELETE FROM clothing_season WHERE clothing_id LIKE '0ffd0e00-0000-0000-0000-%';
DELETE FROM clothing WHERE id LIKE '0ffd0e00-0000-0000-0000-%';
