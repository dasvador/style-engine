-- 룩북에 담은 사진을 룩에 직접 붙여 둔다.
--
-- 지금까지는 `image_prompt` 를 해싱해 `outfit_image` 에서 찾았다. 그런데 캐시 키에는
-- 완성된 이미지 프롬프트 전체의 해시가 들어가서, 자세 문구 하나만 고쳐도 짝이 끊기고
-- 룩북의 사진이 전부 비었다. 파일은 그대로 있는데 사용자에게는 사진이 사라진 것으로
-- 보인다. 담을 때(또는 처음 사진이 붙을 때) 경로를 적어 두면 프롬프트를 고쳐도
-- 담아 둔 사진은 그대로 남는다.
ALTER TABLE lookbook_look
  ADD COLUMN image_path VARCHAR(255) DEFAULT NULL AFTER image_prompt;
