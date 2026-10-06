-- 무드 체계를 다시 나눈다.
--
-- 기존 목록은 세 무드가 서로 겹쳤다.
--   미니멀 클래식 — 단정하고 기본적인 옷
--   모던 시크     — 절제되고 세련된 옷
--   모델 오프듀티 — 모델의 일상복. 미니멀·스트리트·캐주얼 어디로도 나올 수 있다
--
-- 그래서 '모델 오프듀티' 를 없애고 '미니멀 클래식' 을 미니멀과 클래식으로 쪼갠다.
-- 미니멀은 장식을 덜어낸 쪽, 클래식은 셔츠·재킷·트렌치의 단정한 핏 쪽이다.
--
-- 남성 목록도 같은 기준으로 맞춘다. '스마트 캐주얼'(셔츠·니트·슬랙스로 단정하게)은
-- 새 '클래식' 과 정의가 사실상 같아서, 둘을 함께 두면 지금 없애려는 겹침이 남성 쪽에
-- 새로 생긴다. 그래서 스마트 캐주얼 자리를 클래식이 대신하고 남성도 8개를 유지한다.
--
-- 파괴적이지 않다. 아이템을 지우지 않고 태그만 옮기며, 여러 번 실행해도 결과가 같다.

-- ─── 1. 이름만 바뀐 것 ───
UPDATE clothing SET style_mood = 'romantic'      WHERE style_mood = 'romantic_feminine';
UPDATE clothing SET style_mood = 'classic'       WHERE style_mood = 'smart_casual';
UPDATE outfit_image SET mood = 'romantic'        WHERE mood = 'romantic_feminine';
UPDATE outfit_image SET mood = 'classic'         WHERE mood = 'smart_casual';

-- ─── 2. 미니멀 클래식 → 미니멀 / 클래식 ───
--
-- 가르는 기준은 "단정한 핏을 만드는 옷인가" 다. 셔츠·테일러드 재킷·트렌치·슬랙스와
-- 그에 맞는 구두·구조적인 토트는 클래식, 나머지 기본 아이템은 미니멀로 둔다.
-- 이름으로 고르는 이유: 같은 `sub_category` 안에서도 갈리기 때문이다
-- (슬랙스는 클래식, 와이드 팬츠는 미니멀).

UPDATE clothing SET style_mood = 'classic'
  WHERE gender = 'female' AND style_mood = 'minimal_classic'
    AND name IN (
      '화이트 코튼 오버핏 셔츠',
      '베이지 하이웨이스트 슬랙스', '블랙 와이드 슬랙스', '차콜 테이퍼드 울 팬츠',
      '네이비 코튼 트렌치코트', '크림 숏 트렌치코트',
      '라이트베이지 오버핏 블레이저', '블랙 싱글 테일러드 자켓',
      '블랙 레더 스퀘어토 로퍼', '블랙 슬링백 키튼힐',
      '그레이 스트럭처드 토트백'
    );

-- 남은 미니멀 클래식은 전부 미니멀로.
UPDATE clothing SET style_mood = 'minimal' WHERE style_mood = 'minimal_classic';
UPDATE outfit_image SET mood = 'minimal'   WHERE mood = 'minimal_classic';

-- ─── 3. 모델 오프듀티 해체 ───
--
-- 이 장르의 아이템은 기본 아이템이라 셋으로 갈린다. 후디·와이드 데님·트러커·
-- 라이더처럼 볼륨과 캐주얼함이 강한 것은 스트리트, 셔츠·트렌치 계열의 단정한 것은
-- 클래식, 나머지 민무늬 기본 아이템은 미니멀이다.

UPDATE clothing SET style_mood = 'street'
  WHERE gender = 'female' AND style_mood = 'model_off_duty'
    AND name IN (
      '차콜 코튼 후드 스웨트셔츠', '그레이 코튼 후드 집업',
      '인디고 와이드 데님', '인디고 데님 트러커 자켓', '블랙 레더 라이더 자켓'
    );

UPDATE clothing SET style_mood = 'classic'
  WHERE gender = 'female' AND style_mood = 'model_off_duty'
    AND name IN (
      '화이트 코튼 오버사이즈 셔츠', '네이비 스트라이프 롱슬리브',
      '라이트 인디고 스트레이트 데님', '카키 코튼 유틸리티 자켓',
      '탄 스웨이드 로퍼', '블랙 레더 첼시부츠', '블랙 레더 앵클부츠',
      '브라운 레더 토트백', '다크브라운 스웨이드 호보백'
    );

UPDATE clothing SET style_mood = 'minimal' WHERE style_mood = 'model_off_duty';
UPDATE outfit_image SET mood = 'minimal'   WHERE mood = 'model_off_duty';

-- ─── 4. 화면에 나가는 목록 ───
-- 표시명과 설명은 코드가 아니라 여기에 있다. 각 무드에 색·실루엣·소재 기준을
-- 한 줄로 적어, 이름만 보고 고르지 않아도 되게 한다.

UPDATE IGNORE style_mood SET mood_key = 'minimal'  WHERE mood_key = 'minimal_classic';
UPDATE IGNORE style_mood SET mood_key = 'romantic' WHERE mood_key = 'romantic_feminine';
UPDATE IGNORE style_mood SET mood_key = 'classic'  WHERE mood_key = 'smart_casual';

INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('female', 'minimal',        '미니멀',          '장식을 덜어낸 단순한 실루엣과 적은 색상으로 정리한 스타일', 1),
  ('female', 'classic',        '클래식',          '셔츠와 재킷, 트렌치코트를 단정한 핏으로 입는 스타일', 2),
  ('female', 'romantic',       '로맨틱',          '부드러운 소재와 레이스, 곡선적인 실루엣으로 연출한 스타일', 3),
  ('female', 'modern_chic',    '모던 시크',       '선명한 실루엣과 비대칭, 강한 명암 대비로 도시적인 인상을 주는 스타일', 4),
  ('female', 'bohemian',       '보헤미안',        '자연스러운 소재와 패턴을 느슨하게 레이어링한 스타일', 5),
  ('female', 'street',         '스트리트',        '오버사이즈 실루엣과 그래픽, 데님과 카고를 중심으로 한 스타일', 6),
  ('female', 'mannish',        '매니시',          '남성복의 테일러링과 넓은 어깨, 와이드 슬랙스로 구성한 스타일', 7),
  ('female', 'sporty_casual',  '스포티 캐주얼',   '기능성 소재와 트랙 팬츠처럼 운동복에서 온 요소를 살린 스타일', 8),
  ('male',   'minimal',        '미니멀',          '장식을 덜어낸 단순한 실루엣과 적은 색상으로 정리한 스타일', 1),
  ('male',   'classic',        '클래식',          '셔츠와 재킷, 트렌치코트를 단정한 핏으로 입는 스타일', 2),
  ('male',   'amekaji',        '아메카지',        '데님과 치노, 셔츠 등 클래식한 아메리칸 캐주얼을 자연스럽게 조합한 스타일', 3),
  ('male',   'preppy',         '프레피',          '셔츠와 니트, 블레이저를 중심으로 단정하고 경쾌하게 연출한 스타일', 4),
  ('male',   'workwear',       '워크웨어',        '견고한 소재와 실용적인 디테일을 살린 작업복 기반 스타일', 5),
  ('male',   'street',         '스트리트',        '오버사이즈 실루엣과 그래픽, 데님과 카고를 중심으로 한 스타일', 6),
  ('male',   'outdoor_casual', '아웃도어 캐주얼', '기능성 아웃도어 아이템을 일상적인 옷차림에 자연스럽게 활용한 스타일', 7),
  ('male',   'sporty_casual',  '스포티 캐주얼',   '기능성 소재와 트랙 팬츠처럼 운동복에서 온 요소를 살린 스타일', 8)
AS new
ON DUPLICATE KEY UPDATE
  mood_label = new.mood_label,
  description = new.description,
  sort_order = new.sort_order;

-- 새 목록에 없는 행을 정리한다. 아이템은 위에서 이미 옮겨졌으므로 여기서 지워지는
-- 것은 선택지 목록뿐이다.
DELETE FROM style_mood
  WHERE (gender = 'female'
         AND mood_key NOT IN ('minimal', 'classic', 'romantic', 'modern_chic',
                              'bohemian', 'street', 'mannish', 'sporty_casual'))
     OR (gender = 'male'
         AND mood_key NOT IN ('minimal', 'classic', 'amekaji', 'preppy',
                              'workwear', 'street', 'outdoor_casual', 'sporty_casual'));
