-- 옷 하나가 여러 장르에 속할 수 있게 한다.
--
-- `clothing.style_mood` 는 값이 하나뿐이라 한 아이템을 한 장르에만 둘 수 있었다.
-- 실제로는 겹친다 — 옥스퍼드 셔츠는 클래식이면서 아메카지이고 프레피다. 한 곳에만
-- 넣으면 나머지 장르에서는 그 옷이 영영 후보에 들어가지 않는다.
--
-- `source` 를 함께 남기는 이유: 사용자가 직접 고른 장르와 Vision 이 제안한 장르,
-- 속성 규칙이 추론한 장르는 신뢰도가 다르다. 나중에 사용자가 고친 것만 남기거나,
-- 제안이 얼마나 맞았는지 따져 보려면 출처를 알아야 한다.
--
-- `clothing.style_mood` 는 지우지 않는다. 대표 장르로 계속 쓰고(목록에 한 줄로
-- 보여줄 때 필요하다), 후보를 고르는 기준은 이 표가 된다. 아래 백필로 둘을 맞춘다.

CREATE TABLE IF NOT EXISTS clothing_style_genre (
  clothing_id CHAR(36) NOT NULL,
  style_genre VARCHAR(50) NOT NULL,
  -- user: 사용자가 직접 고름 / vision: 이미지 분석이 제안 / rule: 속성 규칙이 추론
  source VARCHAR(10) NOT NULL DEFAULT 'rule',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (clothing_id, style_genre),
  INDEX idx_genre (style_genre)
);

-- 지금 태그가 붙어 있는 아이템을 그대로 옮긴다. 시드가 장르를 하나씩 지정해
-- 넣은 것이므로 출처는 'rule' 로 둔다 — 사용자가 고른 것이 아니다.
INSERT IGNORE INTO clothing_style_genre (clothing_id, style_genre, source)
SELECT id, style_mood, 'rule'
FROM clothing
WHERE style_mood IS NOT NULL AND style_mood <> '';
