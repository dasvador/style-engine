-- 이미지 생성이 provider 안전 검사에 막힌 착장을 캐시에 남긴다(image_path 는 NULL).
-- 같은 착장·같은 프롬프트는 다시 생성하지 않는다 — 차단은 재시도로 바뀌지 않는다. 옷이 바뀌면
-- outfit_hash 가 달라져 새로 요청한다.
ALTER TABLE outfit_image
  MODIFY COLUMN verification_status
    ENUM('passed', 'rejected', 'check_error', 'moderation_blocked')
    NOT NULL DEFAULT 'passed'
    COMMENT 'passed=여성 판정 통과 / rejected=재시도 후에도 통과 못해 이미지를 내보내지 않음 / check_error=검증 API 오류 (캐시하지 않음) / moderation_blocked=생성 안전 검사 차단 (이미지 없음, 다시 생성하지 않음)';
