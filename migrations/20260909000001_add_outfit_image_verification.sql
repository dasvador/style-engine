-- 이미지 캐시에 검증 결과를 함께 남긴다.
--
-- 지금까지는 성별 검증을 통과한 이미지만 캐시에 들어갔다. 통과하지 못한
-- 이미지도 사용자에게 반환하면서 캐시에는 넣지 않았기 때문에, 검증을 계속
-- 통과하지 못하는 착장은 요청할 때마다 이미지를 3장씩 새로 생성했다.
-- 반환할 결과라고 판단했으면 캐시에도 넣어야 한다.
--
-- 대신 어떤 경위로 들어온 결과인지는 구분되어야 한다. 그래야 나중에
-- "검증이 실제로 얼마나 실패하는가" 를 이 테이블에서 집계할 수 있다.
ALTER TABLE outfit_image
  ADD COLUMN verification_status
    ENUM('passed', 'accepted_after_retries', 'check_error')
    NOT NULL DEFAULT 'passed'
    COMMENT 'passed=여성 판정 통과 / accepted_after_retries=재시도 후에도 미통과했으나 사용 / check_error=검증 API 오류로 사용',
  ADD COLUMN generation_attempts INT NOT NULL DEFAULT 1
    COMMENT '최종 이미지를 얻기까지 호출한 이미지 생성 횟수';
