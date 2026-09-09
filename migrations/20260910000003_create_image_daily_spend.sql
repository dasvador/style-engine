-- 이미지 생성 지출 원장.
--
-- 이 앱 비용의 84%가 이미지 생성이다 (실측: 이미지 $0.0123/장, 채팅 $0.0023/턴).
-- 그래서 이미지만 막아도 총액이 잡히고, 채팅·추천은 계속 쓸 수 있다.
--
-- 인메모리 카운터로 두지 않는 이유는 재시작이다. 컨테이너가 재기동하면 상한이
-- 초기화되어 상한이 아니게 된다.
--
-- cost_usd 를 DECIMAL 이 아니라 DOUBLE 로 둔다. 여기 들어가는 값은 단가표에서
-- 계산한 추정치이고 정확한 청구액은 provider 대시보드가 기준이므로, 십진 정밀도가
-- 의미를 갖지 않는다.
CREATE TABLE IF NOT EXISTS image_daily_spend (
  spend_date       DATE   NOT NULL PRIMARY KEY,
  cost_usd         DOUBLE NOT NULL DEFAULT 0 COMMENT '그날 이미지 생성·검증에 쓴 추정 비용',
  generate_calls   INT    NOT NULL DEFAULT 0,
  verify_calls     INT    NOT NULL DEFAULT 0,
  blocked_requests INT    NOT NULL DEFAULT 0 COMMENT '예산 소진으로 이미지를 만들지 않은 요청 수',
  updated_at       DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);
