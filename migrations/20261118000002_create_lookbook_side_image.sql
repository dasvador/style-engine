-- 룩북 사진의 90도 옆모습을 계정·원본 사진별로 한 장만 남긴다.
--
-- 옆모습은 룩북 카드에서 처음 누를 때 원본 착용 사진을 편집(images/edits)해 만든다. 한 장에 3센트쯤
-- 든다. 화면 상태로만 들고 있으면 새로고침·서버 재시작·동시 클릭마다 같은 사진을 다시 산다. 그래서
-- (user_id, source_path) 를 기본 키로 두고, 생성을 시작할 때 이 행을 먼저 'pending' 으로 잡는다 —
-- 행을 잡은 요청만 모델을 부른다(나머지는 기다린다).
--
-- source_path: 원본 사진(`outfit_image.image_path`, 착용 사진만). 룩이 아니라 사진에 묶는다 — 같은 사진을
--   쓰는 룩이 여럿이어도 옆모습은 한 장이다. 원본 파일은 건드리지 않는다.
-- status: pending=만드는 중 / ready=있음 / failed=일시 오류(다시 시도 가능) / blocked=안전 검사 차단
--   (다시 만들지 않음).
-- claim_token·claimed_at: 생성을 잡은 요청. 서버가 생성 도중 꺼지면 pending 이 남는데, 오래된 pending 은
--   다음 요청이 다시 잡는다.
CREATE TABLE IF NOT EXISTS lookbook_side_image (
  user_id VARCHAR(50) NOT NULL,
  source_path VARCHAR(255) NOT NULL,
  status ENUM('pending', 'ready', 'failed', 'blocked') NOT NULL,
  image_path VARCHAR(255) DEFAULT NULL,
  attempts INT NOT NULL DEFAULT 0,
  prompt_text TEXT DEFAULT NULL,
  claim_token CHAR(36) DEFAULT NULL,
  claimed_at DATETIME DEFAULT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (user_id, source_path),
  INDEX idx_side_image_path (image_path)
);
