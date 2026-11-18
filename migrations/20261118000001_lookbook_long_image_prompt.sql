-- 룩북 `image_prompt` 를 600자보다 길게 담는다.
--
-- 2026-11-18 글램 추천 세 장 중 하나(셔링 튜브탑 + 사이드 슬릿 맥시 + 시어링 재킷, 추운 날 여밈 문구 포함)의
-- 이미지 문구가 655자였다. `VARCHAR(600)` 이라 엄격 모드에서 저장이 "Data too long" 으로 실패했고, 화면은
-- 저장 실패를 조용히 넘겨 사진은 생성됐는데 룩북에서만 빠졌다(look_signal 도 남지 않았다).
--
-- 같은 코디를 한 행으로 합치는 유니크 키는 문자열 대신 그 SHA-256 으로 건다 — 긴 문자열은 인덱스 키 길이
-- 제한에 걸린다. 해시는 생성 열이라 앱이 따로 계산하지 않는다. 기존 행의 값은 바뀌지 않는다.

-- 한 문장의 ALTER 로 묶는다 — 중간에 실패해도 유니크 키가 빠진 상태로 남지 않는다.
ALTER TABLE lookbook_look
  DROP INDEX uniq_user_look,
  MODIFY image_prompt TEXT NOT NULL,
  ADD COLUMN image_prompt_hash BINARY(32)
    AS (UNHEX(SHA2(image_prompt, 256))) STORED NOT NULL AFTER image_prompt,
  ADD UNIQUE KEY uniq_user_look (user_id, image_prompt_hash);
