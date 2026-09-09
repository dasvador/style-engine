-- 이미지 캐시에서 두 가지를 물어볼 수 있게 한다.
--
-- 1. 무드별 검증 실패율. 실험에서 실패가 boyish 에 몰렸는데, 그게 실사용에서도
--    그런지 확인하려면 무드가 컬럼이어야 한다. 지금은 prompt_text 앞부분을
--    문자열로 파싱해야 나오고, 그건 집계가 아니라 추측이다.
--
-- 2. 캐시 적중률. 착장당 한 행만 남고 재사용 횟수는 세지 않았다. 그런데 이
--    기능의 실제 비용은 적중률이 결정한다 — 장당 $0.0123 은 캐시 미스일 때 값이다.
ALTER TABLE outfit_image
  ADD COLUMN mood VARCHAR(32) NULL
    COMMENT '이미지를 만들 때 쓴 무드. 무드별 실패율 집계용',
  ADD COLUMN hit_count INT NOT NULL DEFAULT 0
    COMMENT '이 캐시 행이 재사용된 횟수 (생성 1회는 포함하지 않는다)';

-- 기존 행 채우기. 무드마다 프롬프트 첫 문장이 달라서 한 번은 이렇게 복구할 수 있다.
-- 앞으로는 핸들러가 직접 넣으므로 이 파싱은 여기서만 쓰인다.
UPDATE outfit_image SET mood = CASE
  WHEN prompt_text LIKE 'Quiet luxury fashion photo%'                        THEN 'quiet_luxury'
  WHEN prompt_text LIKE 'Coquette balletcore fashion photo%'                 THEN 'coquette'
  WHEN prompt_text LIKE 'Office siren fashion photo%'                        THEN 'office_siren'
  WHEN prompt_text LIKE 'Luxury bohemian fashion photo%'                     THEN 'boho'
  WHEN prompt_text LIKE 'Off-duty model fashion photo%'                      THEN 'off_duty'
  WHEN prompt_text LIKE 'Urban street-style fashion photo%'                  THEN 'street'
  WHEN prompt_text LIKE 'Street-style fashion photo of a young boyish-cool%' THEN 'boyish'
  WHEN prompt_text LIKE 'Street-style fashion photo of a young hipster%'     THEN 'amekaji'
  ELSE NULL
END
WHERE mood IS NULL AND prompt_text IS NOT NULL;

CREATE INDEX idx_mood_status ON outfit_image (mood, verification_status);
