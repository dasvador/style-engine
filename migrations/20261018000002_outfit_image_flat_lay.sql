-- 착용 사진이 안전 검사에 막힌 착장의 대체 이미지(인물 없는 의류 배치)를 같은 캐시에 따로 남긴다.
-- presentation: model=여성 모델 착용 사진(기존 행 전부) / flat_lay=의류 배치. 프롬프트가 달라
-- prompt_hash 도 달라서 같은 착장의 착용 사진 행과 섞이지 않는다.
-- not_applicable: 성별 검증 대상이 아님(의류 배치). passed 로 적지 않는다.
-- generation_error: 의류 배치 생성의 일시적 오류. 영구 차단이 아니며 상한 안에서 다시 시도한다.
ALTER TABLE outfit_image
  ADD COLUMN presentation ENUM('model', 'flat_lay') NOT NULL DEFAULT 'model'
    COMMENT 'model=착용 사진 / flat_lay=인물 없는 의류 배치';

ALTER TABLE outfit_image
  MODIFY COLUMN verification_status
    ENUM('passed', 'rejected', 'check_error', 'moderation_blocked', 'not_applicable', 'generation_error')
    NOT NULL DEFAULT 'passed'
    COMMENT 'passed=여성 판정 통과 / rejected=재시도 후에도 통과 못함 / check_error=검증 API 오류(캐시 안 함) / moderation_blocked=생성 안전 검사 차단(다시 생성 안 함) / not_applicable=의류 배치(성별 검증 대상 아님) / generation_error=의류 배치 일시 오류(상한 안에서 재시도)';
