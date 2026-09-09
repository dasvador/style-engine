-- 남성 스타일 장르를 8개 대표 장르로 재편한다.
--
-- 기존 목록은 아메카지 / 미니멀 캐주얼 / 스트릿 세 개뿐이었다. 출근복, 단정한
-- 데이트룩, 운동복, 아웃도어를 고를 자리가 없어서 전부 아메카지로 흘러들었고,
-- 실제로 마이그레이션 `20260521000002` 는 그 시점의 남성 아이템 전부를
-- `amekaji` 로 태깅했다. 장르가 후보 필터로 쓰이는 구조에서(`clothing_repo::
-- list_clothing_filtered`) 이건 필터가 사실상 동작하지 않는다는 뜻이다.
--
-- 여성 재편(`20260910000004`)과 같은 방식이다. 아이템을 지우거나 컬럼을
-- 떨어뜨리지 않고 식별자만 옮기며, 여러 번 실행해도 결과가 같다.
--
-- 여성 쪽 변경은 건드리지 않는다. 여기서 여성 행에 손대는 곳은 남녀 공용 장르
-- 세 개(minimal_classic / street / sporty_casual)의 정의를 맞추는 부분뿐이다.

-- ─── 1. 아이템 태그 이전 ───
--
-- `minimal`("미니멀 캐주얼")은 이름이 바뀐 것이 아니라 `minimal_classic` 으로
-- 합쳐졌다. 두 장르의 정의(절제된 색상, 간결한 실루엣, 로고 없는 기본 아이템)가
-- 같은 것을 가리키고 있었고, 남녀에 같은 장르를 따로 두면 공용 장르가 성별마다
-- 다른 값이 되어 필터 하나가 다른 쪽 데이터를 통째로 놓친다.
--
-- 여성 행은 `20260910000004` 에서 이미 옮겨졌다. 여기서는 성별 조건 없이 남은
-- 값을 마저 옮긴다 — 이미 옮겨진 행은 조건에 걸리지 않으므로 재실행에 안전하다.

UPDATE clothing SET style_mood = 'minimal_classic'
  WHERE style_mood IN ('minimal', 'minimal_casual', 'quiet_luxury');

UPDATE clothing SET style_mood = 'street'
  WHERE style_mood IN ('streetwear', 'street_style');

-- 아래 두 값은 지금 스키마에 남아 있을 자리가 없지만, 운영 DB 에 직접 넣은 값이
-- 있었던 전례가 있어(`20260910000004` 주석 참고) 함께 정규화한다.
UPDATE clothing SET style_mood = 'outdoor_casual'
  WHERE style_mood IN ('gorpcore', 'outdoor');

UPDATE clothing SET style_mood = 'sporty_casual'
  WHERE style_mood IN ('athleisure', 'sportswear', 'sporty');

-- 생성된 룩북 이미지에 기록된 장르도 같이 옮긴다. 여기는 성별 구분이 없다.
UPDATE outfit_image SET mood = 'minimal_classic' WHERE mood IN ('minimal', 'minimal_casual');
UPDATE outfit_image SET mood = 'street'          WHERE mood IN ('streetwear', 'street_style');
UPDATE outfit_image SET mood = 'outdoor_casual'  WHERE mood IN ('gorpcore', 'outdoor');
UPDATE outfit_image SET mood = 'sporty_casual'   WHERE mood IN ('athleisure', 'sportswear', 'sporty');

-- ─── 2. 화면에 나가는 장르 목록 ───
--
-- 표시명과 설명은 코드가 아니라 이 테이블에 있다. 문구 수정에 배포가 필요 없다.
-- 설명은 광고 문구가 아니라 아이템·실루엣·활용 상황으로 쓴다 — 사용자가 8개
-- 중에서 고르려면 차이가 한 줄에 보여야 한다.

-- 예전 키로 남아 있는 남성 행을 새 키로 옮긴다. UNIQUE(gender, mood_key) 가
-- 걸려 있어 이미 새 키가 있으면 옮기지 않고, 아래 INSERT 가 값을 갱신한다.
UPDATE IGNORE style_mood SET mood_key = 'minimal_classic'
  WHERE gender = 'male' AND mood_key IN ('minimal', 'minimal_casual');

INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('male', 'minimal_classic', '미니멀 클래식',   '절제된 색상과 간결한 실루엣을 중심으로 한 단정한 스타일', 1),
  ('male', 'smart_casual',    '스마트 캐주얼',   '셔츠와 니트, 슬랙스처럼 단정한 아이템을 편안하게 조합한 스타일', 2),
  ('male', 'amekaji',         '아메카지',        '데님과 치노, 셔츠 등 클래식한 아메리칸 캐주얼을 자연스럽게 조합한 스타일', 3),
  ('male', 'preppy',          '프레피',          '셔츠와 니트, 블레이저를 중심으로 단정하고 경쾌하게 연출한 스타일', 4),
  ('male', 'workwear',        '워크웨어',        '견고한 소재와 실용적인 디테일을 살린 작업복 기반 스타일', 5),
  ('male', 'street',          '스트리트',        '그래픽과 여유로운 실루엣, 스니커즈를 중심으로 개성을 표현하는 스타일', 6),
  ('male', 'outdoor_casual',  '아웃도어 캐주얼', '기능성 아웃도어 아이템을 일상적인 옷차림에 자연스럽게 활용한 스타일', 7),
  ('male', 'sporty_casual',   '스포티 캐주얼',   '스포츠웨어를 바탕으로 활동성과 편안함을 살린 일상 스타일', 8)
AS new
ON DUPLICATE KEY UPDATE
  mood_label = new.mood_label,
  description = new.description,
  sort_order = new.sort_order;

-- 새 목록에 없는 남성 장르 행을 정리한다. 아이템은 위에서 이미 새 장르로
-- 옮겨졌으므로 여기서 지워지는 것은 선택지 목록뿐이다.
DELETE FROM style_mood
  WHERE gender = 'male'
    AND mood_key NOT IN (
      'minimal_classic', 'smart_casual', 'amekaji', 'preppy',
      'workwear', 'street', 'outdoor_casual', 'sporty_casual'
    );

-- ─── 3. 남녀 공용 장르의 'unisex' 행 정리 ───
--
-- `20260521000001` 이 넣은 ('unisex','minimal'), ('unisex','street') 두 행은
-- 어느 재편에서도 정리되지 않았다. 조회 쿼리가
-- `WHERE gender = ? OR gender = 'unisex'` 라서 이 행들은 남녀 양쪽 목록에
-- 그대로 따라붙는다. 그 결과가 두 가지다.
--
--  * '스트릿'(unisex)이 성별 행의 '스트리트'와 나란히 떠서 같은 장르가 칩 두 개로
--    보인다.
--  * '미니멀'(unisex)은 아이템이 전부 `minimal_classic` 으로 옮겨간 뒤라 눌러도
--    후보가 0건인 죽은 칩이다.
--
-- 공용 장르는 이제 성별 행으로 각각 존재한다(위 INSERT 와 `20260910000004`).
-- 노출 목록을 성별 행 하나로 일원화하고 중복 행을 지운다. 지워지는 것은 선택지
-- 목록이며, 이 키로 태깅된 아이템은 1번에서 이미 새 장르로 옮겨졌다.
DELETE FROM style_mood WHERE gender = 'unisex' AND mood_key IN ('minimal', 'street');
