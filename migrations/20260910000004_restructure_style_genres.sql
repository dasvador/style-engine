-- 여성 스타일 장르를 8개 대표 장르로 재편한다.
--
-- 기존 목록은 넓은 스타일(미니멀, 스트릿)과 유행성 마이크로 트렌드(퀴엣 럭셔리,
-- 코켓, 오피스 사이렌)가 같은 단계에 섞여 있었다. 이름도 표기가 흔들렸다
-- ("보호 리바이벌"은 boho 의 음차다).
--
-- 이 마이그레이션은 파괴적이지 않다. 아이템을 지우거나 컬럼을 떨어뜨리지 않고,
-- 식별자만 옮긴다. 여러 번 실행해도 결과가 같다.
--
-- 조사 결과 두 가지가 드러났고 여기서 함께 정리한다.
--
-- 1. 운영 DB 의 여성 무드 목록(quiet_luxury, coquette, office_siren, boho,
--    off_duty)이 어떤 마이그레이션에도 없었다. 마이그레이션 밖에서 직접 넣은
--    값이라, 새로 배포하면 개발 환경과 다른 목록이 나온다. 아래 INSERT 로
--    목록 자체를 마이그레이션이 소유하게 만든다.
--
-- 2. `off_duty` 로 태깅된 21개가 전부 `style = '스포츠'` 였다. 스포츠브라·레깅스·
--    러닝화·조거팬츠뿐이고 모델 사복(데님, 가죽 재킷, 티셔츠)은 한 개도 없다.
--    예전 정의가 "모델 사복 + 애슬레저"로 둘을 합쳐 두었기 때문이다. 새 분류는
--    둘을 나누므로, 판단 기준을 이름이 아니라 이미 타입이 붙어 있는 `style`
--    컬럼에 둔다.

-- ─── 1. 아이템 태그 이전 ───
-- 성별을 함께 보고 옮긴다. `minimal` 은 남성/공용에서 계속 쓰는 값이라
-- 여성 행만 `minimal_classic` 으로 옮겨야 한다.

UPDATE clothing SET style_mood = 'minimal_classic'
  WHERE gender = 'female' AND style_mood IN ('quiet_luxury', 'minimal');

UPDATE clothing SET style_mood = 'romantic_feminine'
  WHERE gender = 'female' AND style_mood IN ('coquette', 'feminine_casual');

UPDATE clothing SET style_mood = 'modern_chic'
  WHERE gender = 'female' AND style_mood = 'office_siren';

UPDATE clothing SET style_mood = 'bohemian'
  WHERE gender = 'female' AND style_mood IN ('boho', 'boho_revival', 'vintage');

UPDATE clothing SET style_mood = 'mannish'
  WHERE gender = 'female' AND style_mood = 'boyish';

-- 애슬레저는 스포티 캐주얼로, 나머지 오프듀티는 모델 오프듀티로.
-- 순서가 중요하다: 스포츠를 먼저 빼내야 이름만 보고 뭉뚱그리지 않는다.
UPDATE clothing SET style_mood = 'sporty_casual'
  WHERE gender = 'female' AND style_mood = 'off_duty' AND style = '스포츠';

UPDATE clothing SET style_mood = 'model_off_duty'
  WHERE gender = 'female' AND style_mood = 'off_duty';

-- 생성된 룩북 이미지에도 장르가 기록돼 있다. 같이 옮기지 않으면 장르별 집계가
-- 예전 이름과 새 이름으로 갈라진다. 여기는 성별 구분이 없으므로 여성 전용 키만 옮긴다.
UPDATE outfit_image SET mood = 'minimal_classic'   WHERE mood = 'quiet_luxury';
UPDATE outfit_image SET mood = 'romantic_feminine' WHERE mood IN ('coquette', 'feminine_casual');
UPDATE outfit_image SET mood = 'modern_chic'       WHERE mood = 'office_siren';
UPDATE outfit_image SET mood = 'bohemian'          WHERE mood IN ('boho', 'boho_revival', 'vintage');
UPDATE outfit_image SET mood = 'mannish'           WHERE mood = 'boyish';
-- 이미지 캐시에는 `style` 컬럼이 없어 스포츠 여부를 볼 수 없다. 예전 off_duty
-- 프롬프트가 애슬레저였으므로 그 이미지들은 스포티 캐주얼로 옮긴다.
UPDATE outfit_image SET mood = 'sporty_casual'     WHERE mood = 'off_duty';

-- ─── 2. 화면에 나가는 장르 목록 ───
-- 표시명과 설명은 여기(DB)에 있고 코드에는 없다. 문구 수정에 배포가 필요 없다.
-- 홍보 문구를 쓰지 않는다. 사용자가 장르 간 차이를 한 줄로 구분할 수 있으면 된다.

-- 예전 키로 남아 있는 행을 새 키로 옮긴다. mood_key 에 UNIQUE(gender, mood_key)가
-- 걸려 있어 이미 새 키가 있으면 옮기지 않고 아래 INSERT 가 값을 갱신한다.
UPDATE IGNORE style_mood SET mood_key = 'minimal_classic'
  WHERE gender = 'female' AND mood_key = 'quiet_luxury';
UPDATE IGNORE style_mood SET mood_key = 'romantic_feminine'
  WHERE gender = 'female' AND mood_key IN ('coquette', 'feminine_casual');
UPDATE IGNORE style_mood SET mood_key = 'modern_chic'
  WHERE gender = 'female' AND mood_key = 'office_siren';
UPDATE IGNORE style_mood SET mood_key = 'bohemian'
  WHERE gender = 'female' AND mood_key IN ('boho', 'boho_revival');
UPDATE IGNORE style_mood SET mood_key = 'model_off_duty'
  WHERE gender = 'female' AND mood_key = 'off_duty';
UPDATE IGNORE style_mood SET mood_key = 'mannish'
  WHERE gender = 'female' AND mood_key = 'boyish';

INSERT INTO style_mood (gender, mood_key, mood_label, description, sort_order) VALUES
  ('female', 'minimal_classic',   '미니멀 클래식',  '절제된 색상과 간결한 실루엣을 중심으로 한 단정한 스타일', 1),
  ('female', 'romantic_feminine', '로맨틱 페미닌',  '레이스와 리본, 부드러운 색상으로 여성스러운 분위기를 내는 스타일', 2),
  ('female', 'modern_chic',       '모던 시크',      '모노톤과 슬림한 테일러링으로 도시적인 인상을 주는 스타일', 3),
  ('female', 'bohemian',          '보헤미안',       '자연스러운 소재와 여유로운 실루엣에 패턴과 레이어링을 더한 스타일', 4),
  ('female', 'model_off_duty',    '모델 오프듀티',  '데님과 티셔츠 같은 기본 아이템에 힘을 뺀 자연스러운 스타일', 5),
  ('female', 'street',            '스트리트',       '그래픽과 오버사이즈 실루엣에 스니커즈를 더한 캐주얼 스타일', 6),
  ('female', 'mannish',           '매니시',         '남성복에서 가져온 테일러링과 직선적인 실루엣의 스타일', 7),
  ('female', 'sporty_casual',     '스포티 캐주얼',  '조거 팬츠와 후디처럼 활동성 있는 아이템을 일상복으로 입는 스타일', 8)
AS new
ON DUPLICATE KEY UPDATE
  mood_label = new.mood_label,
  description = new.description,
  sort_order = new.sort_order;

-- 새 목록에 없는 여성 장르 행을 정리한다. 아이템은 위에서 이미 새 장르로
-- 옮겨졌으므로 여기서 지워지는 것은 선택지 목록뿐이다.
DELETE FROM style_mood
  WHERE gender = 'female'
    AND mood_key NOT IN (
      'minimal_classic', 'romantic_feminine', 'modern_chic', 'bohemian',
      'model_off_duty', 'street', 'mannish', 'sporty_casual'
    );
