-- 생성 사진 캐시를 계정별로 나눈다.
--
-- 지금까지는 (outfit_hash, prompt_hash) 하나로 모든 계정이 같은 캐시 행을 썼다. 그래서 B 가 A 와 같은
-- 착장 문구로 요청하면 A 의 사진을 받고, 같은 행을 갱신했다. 캐시 키에 소유자를 넣는다.
-- 기존 행은 지우거나 바꾸지 않는다(모두 지금 user_id 그대로 — 대부분 'default'). 기존 키보다 넓은
-- 키라 기존 행끼리 충돌하지 않는다. 다른 계정은 같은 착장이어도 자기 사진을 새로 만든다(공유 안 함).
ALTER TABLE outfit_image
  DROP INDEX idx_outfit_prompt,
  ADD UNIQUE INDEX idx_user_outfit_prompt (user_id, outfit_hash, prompt_hash);
