-- 반응을 더 분명하게 가른다.
--
-- 지우기에 이유를 받는다. 지금까지 지운 코디는 전부 "졌다"로 셌는데, 지우는 이유에는
-- 코디가 별로인 것 말고도 사진이 이상한 것(다리가 엇갈린 사진처럼)과 그냥 정리하는 것이
-- 섞인다. 엔진은 사진을 모르므로 사진 탓에 지운 것은 엔진 일치율에 잡음이 된다.
--   outfit  — 코디가 별로. 이전 지우기(이유 없음)도 사용자 확인으로 이쪽이다(2026-10-16).
--   photo   — 사진이 이상함. 쌍에서 빼고 사진 품질 문제로 따로 본다.
--   cleanup — 그냥 정리. 쌍에서 뺀다.
--
-- 추천 묶음마다 "이번엔 이게 제일" 을 하나 받는다. 셋 중 하나를 고르면 나머지보다
-- 낫다는 쌍이 바로 생긴다 — 좋아요는 여러 개에 누를 수 있어 서로 비교가 안 된다.
ALTER TABLE look_signal
  ADD COLUMN delete_reason VARCHAR(20) DEFAULT NULL AFTER deleted_at,
  ADD COLUMN best TINYINT(1) NOT NULL DEFAULT 0 AFTER worn_at,
  ADD COLUMN best_at DATETIME DEFAULT NULL AFTER best;

-- 쌍:
--   나은 쪽: 좋아요·입어봄·"제일" 중 하나를 받고 지우지 않은 코디
--   못한 쪽: 코디가 별로라 지운 코디(strong)
--            "제일" 이 있는 묶음에서 고르지 않은 나머지 코디(best)
--            아무 반응도 없는 코디(weak)
-- 사진이나 정리 때문에 지운 코디는 어느 쪽에도 넣지 않는다.
CREATE OR REPLACE VIEW look_pair AS
SELECT b.user_id, b.request_id, b.gender, b.mood_key, b.weather_summary,
       b.look_id AS better_look_id, b.outfit_json AS better_outfit, b.position AS better_position,
       w.look_id AS worse_look_id, w.outfit_json AS worse_outfit, w.position AS worse_position,
       CASE WHEN w.deleted = 1 THEN 'strong' WHEN b.best = 1 THEN 'best' ELSE 'weak' END
         AS strength,
       b.created_at,
       b.scoring_json AS better_scoring, w.scoring_json AS worse_scoring
FROM look_signal b
JOIN look_signal w
  ON w.request_id = b.request_id AND w.user_id = b.user_id AND w.look_id <> b.look_id
WHERE b.deleted = 0 AND (b.liked = 1 OR b.worn = 1 OR b.best = 1)
  AND (
    (w.deleted = 1 AND COALESCE(w.delete_reason, 'outfit') = 'outfit')
    OR (w.deleted = 0 AND w.liked = 0 AND w.worn = 0 AND w.best = 0)
    OR (b.best = 1 AND w.best = 0 AND w.deleted = 0)
  );
