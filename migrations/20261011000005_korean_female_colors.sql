-- 여성 의류의 색 표기를 한국어로 바꾼다.
--
-- 남성 154벌과 Vision 분석 응답은 색을 한국어로 적는다(`블랙`, `오프화이트`). 여성 176벌만
-- 영어 snake_case 였다(`black`, `off_white`). 엔진은 색을 한국어 부분 문자열로 읽는다 —
-- `outfit_scorer::color_group` 은 "네이비" · "블랙" · "크림" 을 찾고, 스타일 엔진의 앵커
-- 판정은 "인디고" · "블랙" · "다크" 를 찾는다. 그래서 여성 아이템은 전부 색상군 "other"
-- 로 떨어져, 같은 색 반복 감점 · 이너와 아우터 동색 감점 · 어두운 하의 앵커 판정이 여성
-- 착장에서 한 번도 작동하지 않았다.
--
-- 남성 어휘에 있는 이름은 그대로 쓴다(크림, 오트밀, 오프화이트, 헤더그레이, 워시드블랙,
-- 라이트블루 …). 남성에게 없는 색은 같은 방식으로 옮겼다(베이비핑크, 라벤더, 머스타드 …).
--
-- 영어 값인 행만 바꾸므로 여러 번 실행해도 결과가 같다.

UPDATE clothing
SET color = CASE color
    WHEN 'black'        THEN '블랙'
    WHEN 'washed_black' THEN '워시드블랙'
    WHEN 'charcoal'     THEN '차콜'
    WHEN 'dark_gray'    THEN '다크그레이'
    WHEN 'gray'         THEN '그레이'
    WHEN 'light_gray'   THEN '라이트그레이'
    WHEN 'heather_gray' THEN '헤더그레이'
    WHEN 'silver'       THEN '실버'
    WHEN 'white'        THEN '화이트'
    WHEN 'off_white'    THEN '오프화이트'
    WHEN 'ivory'        THEN '아이보리'
    WHEN 'cream'        THEN '크림'
    WHEN 'oatmeal'      THEN '오트밀'
    WHEN 'natural'      THEN '내추럴'
    WHEN 'beige'        THEN '베이지'
    WHEN 'light_beige'  THEN '라이트베이지'
    WHEN 'nude_beige'   THEN '누드베이지'
    WHEN 'tan'          THEN '탄'
    WHEN 'camel'        THEN '카멜'
    WHEN 'brown'        THEN '브라운'
    WHEN 'dark_brown'   THEN '다크브라운'
    WHEN 'khaki'        THEN '카키'
    WHEN 'olive'        THEN '올리브'
    WHEN 'mustard'      THEN '머스타드'
    WHEN 'terracotta'   THEN '테라코타'
    WHEN 'navy'         THEN '네이비'
    WHEN 'dark_blue'    THEN '다크블루'
    WHEN 'indigo'       THEN '인디고'
    WHEN 'light_indigo' THEN '라이트인디고'
    WHEN 'medium_blue'  THEN '미디엄블루'
    WHEN 'light_blue'   THEN '라이트블루'
    WHEN 'pink'         THEN '핑크'
    WHEN 'baby_pink'    THEN '베이비핑크'
    WHEN 'soft_pink'    THEN '소프트핑크'
    WHEN 'light_pink'   THEN '라이트핑크'
    WHEN 'nude_pink'    THEN '누드핑크'
    WHEN 'lavender'     THEN '라벤더'
    WHEN 'mint'         THEN '민트'
    ELSE color
  END
WHERE gender = 'female'
  AND color REGEXP '^[a-z_]+$';
