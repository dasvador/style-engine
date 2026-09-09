-- 워크웨어 장르가 여름에 입을 것이 하나도 없는 문제를 고친다.
--
-- 여성 계절 데이터를 채우면서 새로 넣은 점검(`tests/genre_candidates.rs` 의
-- `no_genre_is_stranded_in_a_season`)이 잡아낸 것으로, 남성 시드 데이터의 문제다.
--
-- 워크웨어 후보 중 상의·하의·신발에 '여름'이 붙은 아이템이 한 벌도 없었다.
-- 계절 게이트는 계절 정보가 있는 슬롯의 80% 이상이 비수기면 코디를 떨어뜨리므로
-- (`style_engine_v2::detect_season_complete_mismatch`), 셋 다 비수기가 되어
-- 비율이 1.0 이 된다. 여름에 워크웨어를 고르면 코디가 만들어지지 않는다.
--
-- 옷을 새로 넣지 않고 계절만 바로잡는다. 아래 두 종류는 실제로 여름 아이템이다.
--
-- 여러 번 실행해도 결과가 같다.

-- 샴브레이는 가벼운 여름 셔츠 원단이다. `texture_keywords` 가 이미
-- "chambray, light denim-like woven cotton" 이라고 적고 있는데도 봄·가을만
-- 붙어 있었다. 이름이 아니라 이 키워드로 찾는다.
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT id, '여름' FROM clothing
WHERE gender = 'male'
  AND texture_keywords LIKE '%chambray%';

-- 와이드 치노는 통이 넓어 여름에 입는다. 워크웨어 후보에 드는 치노만 손댄다
-- (`style = '워크'`), 아메카지 쪽 치노는 그대로 둔다.
INSERT IGNORE INTO clothing_season (clothing_id, season)
SELECT id, '여름' FROM clothing
WHERE gender = 'male'
  AND style = '워크'
  AND sub_category = 'chino'
  AND thickness <> 'thick';
