-- 여성 아이템에 계절을 채운다.
--
-- 여성 옷장은 계절이 한 벌도 없었다(남성 140벌은 전부 있다). 계절 게이트는 값이
-- 없는 슬롯을 건너뛰므로(`style_engine_v2::detect_season_complete_mismatch` 와
-- `score_utility` 의 `if slot.seasons.is_empty() { continue; }`), 지금 여성
-- 옷장에서는 계절 판단이 아예 작동하지 않는다. 한겨울에 새틴 미디 스커트와
-- 스포츠 브라탑이 아무 저항 없이 추천될 수 있다는 뜻이다.
--
-- 판단 근거는 두께 하나가 아니라 **아이템 종류 + 소재 + 두께**를 함께 본다.
-- 두께만 보면 린넨 슬랙스(medium)가 가을옷이 되고 얇은 윈드브레이커(thin)가
-- 여름옷이 된다. 애매하면 좁히지 않고 넓게 준다 — 계절을 잘못 좁히면 그 아이템은
-- 해당 계절 추천에서 조용히 사라지지만, 넓게 주면 게이트가 덜 민감해질 뿐이다.
--
-- 아래 CASE 는 위에서부터 먼저 걸리는 규칙이 이긴다. 어디에도 걸리지 않으면
-- ELSE 의 봄·가을이다 — 미디엄 두께의 셔츠·데님·치노·재킷이 여기 해당한다.
--
-- 여러 번 실행해도 결과가 같다(INSERT IGNORE + 복합 PK).

INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT c.id, s.season
FROM clothing c
CROSS JOIN (
  SELECT '봄' AS season
  UNION ALL SELECT '여름'
  UNION ALL SELECT '가을'
  UNION ALL SELECT '겨울'
) s
WHERE c.gender = 'female'
  AND FIND_IN_SET(
    s.season,
    CASE
      -- ─── 사계절 ───
      -- 가방은 계절을 타지 않는다.
      WHEN c.category = '가방' THEN '봄,여름,가을,겨울'
      -- 스니커즈와 러닝화도 마찬가지다.
      WHEN c.sub_category IN ('스니커즈', '러닝화') THEN '봄,여름,가을,겨울'

      -- ─── 신발 ───
      WHEN c.sub_category IN ('글래디에이터샌들', '뮬', '스포츠샌들', '에스파드리유')
        THEN '봄,여름'
      WHEN c.sub_category IN ('웨스턴부츠', '앵클부츠') THEN '가을,겨울'
      -- 발이 덮이는 플랫·로퍼는 스니커즈와 같이 계절을 타지 않는다. 봄가을로
      -- 좁히면 모던 시크와 로맨틱 페미닌은 겨울에 신을 신발이 한 켤레도 없어진다.
      WHEN c.sub_category IN ('로퍼', '플랫슈즈', '메리제인', '발레리나', '슬링백')
        THEN '봄,여름,가을,겨울'

      -- ─── 소재가 계절을 결정하는 경우 ───
      -- 린넨은 두께와 무관하게 여름 소재다. 린넨 블레이저·슬랙스·셔츠자켓이
      -- medium 이라는 이유로 가을옷이 되면 안 된다.
      WHEN c.material_primary = 'linen' THEN '봄,여름'

      -- ─── 겉옷 ───
      -- 니트 가디건은 니트지만 봄가을 겉옷이다. 아래 니트 규칙보다 먼저 본다.
      WHEN c.sub_category IN ('가디건', '카디건', '크로셰가디건') THEN '봄,가을'
      WHEN c.sub_category IN ('코트', '필드자켓') THEN '가을,겨울'
      WHEN c.category = '아우터' AND c.thickness = 'thick' THEN '가을,겨울'

      -- ─── 니트·모직 ───
      -- 울·캐시미어·트위드는 아이템 종류와 무관하게 추운 계절이다
      -- (울 슬랙스, 울혼방 숏 자켓, 트위드 자켓, 캐시미어 터틀넥).
      WHEN c.sub_category IN ('니트', '니트탑', '하프집업니트') THEN '가을,겨울'
      WHEN c.material_primary IN ('knit', 'wool', 'cashmere', 'tweed', 'acrylic')
        THEN '가을,겨울'
      -- 스웨이드 하의는 겨울 쪽이다. 스웨이드 프린지 재킷은 보헤미안 봄가을
      -- 아이템이라 여기 포함하지 않는다.
      WHEN c.material_primary = 'suede' AND c.category = '하의' THEN '가을,겨울'

      -- ─── 레이어링 상의 ───
      -- 긴팔은 얇아도 여름옷이 아니다. 아래 '얇으면 봄여름' 규칙보다 먼저 본다.
      WHEN c.sub_category IN ('롱슬리브', '긴팔티셔츠') THEN '봄,가을'
      -- 맨투맨·후드는 코트 안에 껴입으므로 겨울까지 넓게 준다.
      WHEN c.sub_category IN ('맨투맨', '후드티', '후드집업', '조거팬츠')
        THEN '봄,가을,겨울'

      -- ─── 얇은 상·하의 ───
      --
      -- 두께만으로 가르면 얇은 블라우스가 여름옷이 된다. 실제로는 재킷 안에
      -- 겹쳐 입는 가을 아이템이기도 하다 — 보헤미안 상의 세 벌이 전부 얇은
      -- 블라우스라, 여름옷으로 묶으면 그 장르는 가을에 입을 상의가 없어진다.
      --
      -- 그래서 소매를 기준으로 가른다. 민소매·반팔은 더울 때만 입고, 그 밖의
      -- 얇은 상의와 스커트는 가을까지 간다.
      WHEN c.sub_category IN (
        '탱크탑', '브라탑', '크롭탑', '크롭티셔츠', '반팔티셔츠', '숏팬츠', '숏레깅스'
      ) THEN '봄,여름'
      -- 레깅스는 애슬레저 아이템이라 추울 때 오히려 더 입는다. 한여름만 뺀다.
      WHEN c.sub_category = '레깅스' THEN '봄,가을,겨울'
      WHEN c.thickness = 'thin' AND c.category IN ('상의', '하의') THEN '봄,여름,가을'
      -- 티셔츠·폴로는 중간 두께여도 여름에 입는다.
      WHEN c.sub_category IN ('티셔츠', '폴로셔츠') THEN '봄,여름,가을'

      -- ─── 나머지 ───
      -- 미디엄 두께의 셔츠·티셔츠·데님·치노·카고·와이드팬츠·폴리 슬랙스와
      -- 데님자켓·트렌치·블레이저·윈드브레이커·트랙재킷 같은 봄가을 겉옷.
      ELSE '봄,가을'
    END
  ) > 0;
