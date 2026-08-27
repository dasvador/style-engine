-- thickness 어휘를 선언된 계약(thin/medium/thick)으로 정규화하고 DB에서 강제한다.
--
-- 앞선 20260827000001 이 role/tone/style/weight/saturation 을 정리할 때 thickness 를
-- 빠뜨렸다. 같은 2026-05 여성 시드가 이 컬럼에도 다른 어휘를 넣었다:
--
--   male   : medium 137 / thick 2 / thin 1     ← 선언된 계약
--   female : 중간 97 / 얇은 39 / 두꺼운 5      ← 이탈
--
-- 이 컬럼은 장식이 아니라 온도 게이트가 실제로 비교한다:
--
--   serving_ranker::compute_today_fit          thickness == Thin
--   recommendation_service 적합도 게이트        thickness == Thin
--
-- 'thin' 이 281건 중 1건뿐이었으므로, "얇은 상의 + 아우터 없음 → 온도 부적합" 판정이
-- 한국어 값을 가진 39건에 대해 한 번도 발동하지 못했다.
--
-- 정규화 방향이 영어인 이유는 이 필드의 계약이 처음부터 그랬기 때문이다 —
-- SYSTEM_ARCHITECTURE 의 데이터 모델, 등록 폼의 option value, Vision 프롬프트가 모두
-- thin/medium/thick 을 쓰고 UI 는 표시할 때만 한국어로 옮긴다.

UPDATE clothing SET thickness = 'thin'   WHERE thickness = '얇은';
UPDATE clothing SET thickness = 'medium' WHERE thickness = '중간';
UPDATE clothing SET thickness = 'thick'  WHERE thickness = '두꺼운';

-- 표시용 표현이 값으로 새어들어온 경우까지 방어.
UPDATE clothing SET thickness = 'medium' WHERE thickness = '보통';

ALTER TABLE clothing
    ADD CONSTRAINT chk_clothing_thickness
        CHECK (thickness IN ('thin', 'medium', 'thick'));
