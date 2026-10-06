-- 여성 의류의 실루엣 볼륨을 채운다.
--
-- 여성 176벌은 `silhouette_volume` 이 전부 NULL 이다. 무드끼리의 차이를 실루엣으로
-- 보려면 이 값이 있어야 한다 — 클래식과 모던 시크가 같은 화이트 셔츠를 써도 와이드
-- 슬랙스와 스트레이트 데님은 다른 실루엣이라는 판단이 여기서 나온다.
--
-- 여성 아이템은 이미지가 한 장도 없어서(176/176) Vision 재분석 경로를 쓸 수 없다.
-- 쓸 수 있는 근거는 이름과 `sub_category` 뿐이고, 둘 다 핏을 꽤 직접적으로 적고 있다.
--
-- 판정은 **아래에서 위 순서로 먼저 걸리는 규칙이 이긴다**:
--   1. `sub_category` 가 핏을 결정하는 품목 (레깅스·브라탑·탱크탑·트렌치코트…)
--   2. 이름에 들어간 핏 단어 (오버사이즈·와이드·슬림·테이퍼드·스트레이트…)
--   3. 어디에도 걸리지 않으면 regular
--
-- 이름은 `CONCAT(' ', name, ' ')` 으로 감싸 **토큰 단위**로만 본다. 부분 문자열로
-- 비교하면 `리브` 가 `올리브`·`퍼프슬리브`·`리본` 안에서 걸려 필드자켓이 slim 이 되고,
-- 그 오류는 아무 데서도 드러나지 않는다 (규칙에 걸리기만 할 뿐 오류로 보이지 않는다).
-- `오버사이즈` · `오버핏` · `와이드` 는 `와이드팬츠` 처럼 붙여 쓴 품목명이 있어 토큰
-- **시작** 일치로 둔다.
--
-- 길이·장식·소재를 나타내는 말은 일부러 제외했다 — `맥시`(길이), `프린지`·`크로셰`
-- (장식), `크링클`(질감) 은 볼륨과 다른 축이고, 넣으면 근거 없는 값이 섞인다.
--
-- 신발과 가방은 비워 둔다. 볼륨은 옷에만 의미가 있다. (남성 154벌은 가방·신발까지
-- 전부 `regular` 인데, 그쪽은 실제 판단이라기보다 일괄 기본값으로 보인다.)
--
-- 값이 이미 있는 행은 건드리지 않으므로 여러 번 실행해도 결과가 같다.

UPDATE clothing
SET silhouette_volume = CASE
  -- ─── 1. 품목이 핏을 결정하는 경우 ───
  WHEN sub_category IN ('레깅스', '숏레깅스', '브라탑', '탱크탑') THEN 'slim'
  WHEN sub_category IN ('트렌치코트', '트러커자켓', '플레어진') THEN 'relaxed'

  -- ─── 2. 이름의 핏 단어 (토큰 시작 일치) ───
  WHEN CONCAT(' ', name, ' ') LIKE '% 오버사이즈%' THEN 'oversized'
  WHEN CONCAT(' ', name, ' ') LIKE '% 오버핏%' THEN 'oversized'
  WHEN CONCAT(' ', name, ' ') LIKE '% 와이드%' THEN 'oversized'

  -- ─── 2b. 이름의 핏 단어 (토큰 완전 일치) ───
  WHEN CONCAT(' ', name, ' ') LIKE '% 슬림 %'
    OR CONCAT(' ', name, ' ') LIKE '% 슬림핏 %'
    OR CONCAT(' ', name, ' ') LIKE '% 스키니 %'
    OR CONCAT(' ', name, ' ') LIKE '% 테이퍼드 %'
    OR CONCAT(' ', name, ' ') LIKE '% H라인 %'
    OR CONCAT(' ', name, ' ') LIKE '% 리브 %'
    OR CONCAT(' ', name, ' ') LIKE '% 리브드 %' THEN 'slim'

  WHEN CONCAT(' ', name, ' ') LIKE '% 스트레이트 %'
    OR CONCAT(' ', name, ' ') LIKE '% 릴랙스 %'
    OR CONCAT(' ', name, ' ') LIKE '% 루즈 %'
    OR CONCAT(' ', name, ' ') LIKE '% 보이프렌드 %'
    OR CONCAT(' ', name, ' ') LIKE '% 플레어 %' THEN 'relaxed'

  -- ─── 3. 기본 ───
  ELSE 'regular'
END
WHERE gender = 'female'
  AND category IN ('상의', '하의', '아우터')
  AND silhouette_volume IS NULL;
