-- 여성 의류의 채도를 채운다.
--
-- 여성 176벌 중 141벌이 `saturation` 이 NULL 이었다. 이 값이 없으면 톤·채도 대비
-- 규칙(`style_engine::rule_contrast`)이 아예 돌지 않는다 — 그 규칙은 톤과 채도를
-- **둘 다** 가진 아이템만 모아서 2개 미만이면 바로 돌아가고, 여성 아이템은 톤은
-- 전부 있지만 채도가 없어서 그 문턱을 넘지 못했다. 전부 중간톤인 착장이 아무 저항
-- 없이 통과해 온 이유다.
--
-- ─── 분류 근거 ───
--
-- 색 이름으로 채도를 정하는 것은 함수 관계가 아니다. 남성 데이터에서는 같은
-- `올리브` · `네이비` · `인디고` · `카키` 가 낮음과 중간 양쪽에 있다 — 원단과
-- 워싱을 보고 아이템마다 판단한 결과다. 여성 아이템은 이미지가 없어 그 판단을
-- 재현할 수 없으므로 색 이름 단위로 정리하고, 기준은 **크로마**(색의 선명함)로
-- 잡았다. 색이 있는지가 아니라 얼마나 짙은지를 본다.
--
-- 이 규칙은 이미 값이 있는 여성 35벌과 어긋나지 않는다. 그쪽은
-- `indigo` · `light_indigo` 가 중간이고 나머지(black, white, cream, charcoal,
-- gray, navy, tan, khaki, brown, oatmeal, washed_black, dark_brown, dark_gray,
-- heather_gray)가 전부 낮음인데, 아래 분류와 같다.
--
--   낮음 — 무채색, 오프화이트, 눌린 어스톤, 짙지만 탁한 색, 그리고 파스텔.
--          파스텔을 낮음에 두는 이유: 밝기가 높고 크로마는 낮다. 색이 보인다는
--          이유로 중간에 올리면 베이비핑크가 머스타드와 같은 칸에 들어간다.
--   중간 — 색이 분명히 읽히는 중간 크로마. 올리브 · 카멜 · 테라코타 · 머스타드 ·
--          인디고 · 미디엄블루 · 핑크.
--   높음 — 없다. 이 옷장에는 로얄블루나 브릭레드 같은 선명한 색이 없다.
--          남성도 154벌 중 3벌뿐이다.
--
-- ─── 목록에 없는 색은 비워 둔다 ───
--
-- 기본값을 낮음으로 두지 않는다. 지금 여성 옷장에 있는 색은 아래에 전부 적혀
-- 있으므로 남는 것이 없지만, 나중에 선명한 색이 추가되면 조용히 낮음이 되는 대신
-- NULL 로 남아 눈에 띄는 쪽이 낫다.
--
-- 참고: 여성 `color` 는 영어 snake_case 이고 남성은 한국어다(`블랙` / `black`).
-- 아래 목록이 영어인 이유이고, 그 불일치 자체는 여기서 건드리지 않는다.
--
-- 값이 이미 있는 행은 건드리지 않으므로 여러 번 실행해도 결과가 같다.

UPDATE clothing
SET saturation = CASE
  -- ─── 중간: 색이 분명히 읽히는 중간 크로마 ───
  WHEN color IN (
    'olive', 'camel', 'terracotta', 'mustard', 'pink',
    'indigo', 'light_indigo', 'medium_blue'
  ) THEN '중간'

  -- ─── 낮음: 무채색 ───
  WHEN color IN (
    'black', 'washed_black', 'charcoal', 'dark_gray', 'gray',
    'light_gray', 'heather_gray', 'silver'
  ) THEN '낮음'
  -- 오프화이트 계열
  WHEN color IN ('white', 'off_white', 'ivory', 'cream', 'oatmeal', 'natural')
    THEN '낮음'
  -- 눌린 어스톤
  WHEN color IN (
    'beige', 'light_beige', 'nude_beige', 'tan', 'brown', 'dark_brown', 'khaki'
  ) THEN '낮음'
  -- 짙지만 탁한 색
  WHEN color IN ('navy', 'dark_blue', 'dark_olive') THEN '낮음'
  -- 파스텔 — 밝고 크로마는 낮다
  WHEN color IN (
    'light_blue', 'baby_pink', 'soft_pink', 'light_pink', 'nude_pink',
    'lavender', 'mint'
  ) THEN '낮음'

  -- 목록에 없는 색은 그대로 둔다.
  ELSE NULL
END
WHERE gender = 'female'
  AND saturation IS NULL;
