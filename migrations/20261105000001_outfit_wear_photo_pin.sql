-- 코디 기록에 룩과 사진을 저장 때 고정한다.
--
-- 예전에는 기록을 열 때마다 옷 구성이 같은 룩을 찾아 사진을 골랐다. 그러면 나중에 같은 구성의 룩이 생기면
-- 기록의 사진이 바뀐다. 이제 저장할 때 정확한 룩 id 와 그때의 사진 경로를 값으로 남기고, 조회는 이 값만 쓴다.
--
-- 룩이 지워져도 값은 남긴다(외래 키를 걸지 않는다). 지워졌다는 것은 화면이 안내한다.
--
-- photo_basis — 이 사진이 어떻게 붙었는가:
--   at_save   저장할 때 룩에 이미 있던 사진
--   later     저장할 때는 사진이 없었고, 고정된 그 룩에 나중에 처음 생긴 사진. 다른 룩의 사진은 쓰지 않는다
--   migrated  이 기능 전에 저장한 기록을 한 번 이관한 사진. 저장 당시의 사진이라는 근거는 없다
--
-- 컬럼만 더한다. 기존 행은 건드리지 않는다.

ALTER TABLE outfit_wear
  ADD COLUMN pinned_look_id CHAR(36) DEFAULT NULL,
  ADD COLUMN pinned_image_path VARCHAR(255) DEFAULT NULL,
  ADD COLUMN photo_basis VARCHAR(10) DEFAULT NULL,
  ADD COLUMN photo_pinned_at DATETIME DEFAULT NULL,
  ADD CONSTRAINT chk_wear_photo_basis CHECK (
    (pinned_image_path IS NULL AND photo_basis IS NULL)
    OR (pinned_image_path IS NOT NULL AND photo_basis IN ('at_save', 'later', 'migrated'))
  );
