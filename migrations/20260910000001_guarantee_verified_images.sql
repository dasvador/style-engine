-- 여성 모델을 보장한다: 검증을 통과하지 못한 이미지는 반환하지 않는다.
--
-- 지금까지는 3회 검증에 실패해도 마지막 이미지를 그대로 내보냈다. 보장이
-- 제품 요구사항이라면 그 경로는 있으면 안 된다. 대신 "이 착장은 통과하지
-- 못했다"는 사실 자체를 캐시해야 한다 — 그러지 않으면 실패하는 착장이
-- 요청마다 이미지를 다시 생성한다.
--
-- 그래서 image_path 가 비어 있는 행이 생긴다: 결과는 있지만 내보낼 이미지가
-- 없는 상태다.
ALTER TABLE outfit_image
  MODIFY COLUMN image_path VARCHAR(255) NULL
    COMMENT '검증을 통과한 이미지 경로. 통과하지 못했으면 NULL';

-- accepted_after_retries 는 "검증에 실패했지만 내보낸 이미지"였다. 그런 이미지가
-- 존재하면 안 되므로 캐시에서 지운다 (해당 상태의 행은 현재 없다).
DELETE FROM outfit_image WHERE verification_status = 'accepted_after_retries';

ALTER TABLE outfit_image
  MODIFY COLUMN verification_status
    ENUM('passed', 'rejected', 'check_error')
    NOT NULL DEFAULT 'passed'
    COMMENT 'passed=여성 판정 통과 / rejected=재시도 후에도 통과 못해 이미지를 내보내지 않음 / check_error=검증 API 오류 (캐시하지 않음)';

-- generation_attempts 는 이제 이 착장에 대해 **누적으로** 시도한 횟수다.
-- 통과하지 못한 착장을 무한히 재시도하지 않도록 상한을 두는 데 쓴다.
ALTER TABLE outfit_image
  MODIFY COLUMN generation_attempts INT NOT NULL DEFAULT 1
    COMMENT '이 착장에 대해 누적으로 호출한 이미지 생성 횟수';
