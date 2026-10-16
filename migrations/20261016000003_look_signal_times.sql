-- 반응이 언제 있었는지와, 지운 코디의 사진을 남긴다.
--
-- 시간: 지운 코디로 감점을 줬다면 어땠을지 시간 순서대로 재현하려 했는데, 지운 시각이
-- 없어 저장 시각으로 어림해야 했다. 좋아요·입어봄도 같은 룩이 나중 요청에 다시 나오면
-- 어느 요청에서 누른 것인지 가릴 수 없었다.
--
-- 사진: 화면에서 고르는 것은 코디 목록이 아니라 사진이다. 다리가 엇갈린 사진처럼 그림이
-- 나빠서 지운 경우를 가려내려면 그때의 사진이 있어야 하는데, 룩북에서 지우면 사진
-- 경로를 들고 있던 `lookbook_look` 행이 사라진다. 지울 때 경로를 옮겨 둔다.
ALTER TABLE look_signal
  ADD COLUMN liked_at DATETIME DEFAULT NULL AFTER liked,
  ADD COLUMN worn_at DATETIME DEFAULT NULL AFTER worn,
  ADD COLUMN deleted_at DATETIME DEFAULT NULL AFTER deleted,
  ADD COLUMN image_path VARCHAR(255) DEFAULT NULL AFTER scoring_json;
