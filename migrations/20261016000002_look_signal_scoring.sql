-- 홈 추천이 그 코디에 매긴 실제 점수를 같이 남긴다.
--
-- 반응 쌍 30개로 재 보니 엔진 점수만으로는 고른 쪽과 48~50% 일치했는데, 화면 순서는
-- 60% 였다. 화면 순서에는 이력 감점·다양성·휴면 보너스와 피드백이 섞여 있는데, 테스트는
-- 이것들 없이 재기 때문에 무엇이 맞혔는지 알 수 없었다. 추천 응답의 점수 내역
-- (`ScoringDetail`)과 shadow 점수를 저장 때 함께 받아 둔다.
ALTER TABLE look_signal
  ADD COLUMN scoring_json JSON DEFAULT NULL AFTER outfit_json;

CREATE OR REPLACE VIEW look_pair AS
SELECT b.user_id, b.request_id, b.gender, b.mood_key, b.weather_summary,
       b.look_id AS better_look_id, b.outfit_json AS better_outfit, b.position AS better_position,
       w.look_id AS worse_look_id, w.outfit_json AS worse_outfit, w.position AS worse_position,
       IF(w.deleted = 1, 'strong', 'weak') AS strength,
       b.created_at,
       b.scoring_json AS better_scoring, w.scoring_json AS worse_scoring
FROM look_signal b
JOIN look_signal w
  ON w.request_id = b.request_id AND w.user_id = b.user_id AND w.look_id <> b.look_id
WHERE (b.liked = 1 OR b.worn = 1) AND b.deleted = 0
  AND (w.deleted = 1 OR (w.liked = 0 AND w.worn = 0));
