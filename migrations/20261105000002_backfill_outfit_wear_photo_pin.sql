-- 사진 고정 전에 저장한 기록을 한 번 이관한다. 아래가 확인될 때만 연결하고, 애매하면 연결하지 않는다.
--
--   1. 룩북에서 만든 참조: 참조에 적힌 그 룩이 아직 있을 때. 사진이 없어도 룩은 연결한다.
--   2. 추천·바꾸기 참조: 같은 계정에서 옷 id 구성과 무드가 모두 같은 룩이 정확히 하나이고, 그 룩의 사진이 내 사진일
--      때만. 이름은 보지 않는다. 옷 id 가 빠진 예전 룩은 후보가 아니다.
--
-- 붙은 사진은 photo_basis = 'migrated' 다. 지금 룩에 붙어 있는 사진이지, 저장 당시의 사진이라는 뜻이 아니다.
-- 상태·날짜·revision·updated_at 은 바꾸지 않는다. 이미 연결된 행은 건드리지 않아 다시 돌려도 결과가 같다.

UPDATE outfit_wear w
JOIN (
  SELECT r.id AS ref_id, l.id AS look_id, 0 AS needs_photo
  FROM outfit_ref r
  JOIN lookbook_look l
    ON l.id = SUBSTRING(r.dedupe_key, 6) AND l.user_id = r.user_id
  WHERE r.source = 'lookbook' AND r.dedupe_key LIKE 'look:%'

  UNION ALL

  SELECT r.id AS ref_id, MIN(k.look_id) AS look_id, 1 AS needs_photo
  FROM outfit_ref r
  JOIN (
    SELECT
      l.id AS look_id,
      l.user_id,
      l.mood_key,
      GROUP_CONCAT(DISTINCT j.item_id ORDER BY j.item_id SEPARATOR '|') AS item_key
    FROM lookbook_look l,
      JSON_TABLE(
        l.outfit_json, '$[*]'
        COLUMNS (item_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin PATH '$.id')
      ) j
    GROUP BY l.id, l.user_id, l.mood_key, l.outfit_json
    HAVING COUNT(j.item_id) = JSON_LENGTH(l.outfit_json)
  ) k
    ON k.user_id = r.user_id
    AND k.item_key = CONVERT(r.item_key USING utf8mb4) COLLATE utf8mb4_bin
    AND k.mood_key <=> IF(
      JSON_TYPE(JSON_EXTRACT(r.context_json, '$.mood_key')) = 'STRING',
      JSON_UNQUOTE(JSON_EXTRACT(r.context_json, '$.mood_key')),
      NULL
    ) COLLATE utf8mb4_bin
  WHERE r.source <> 'lookbook'
  GROUP BY r.id
  HAVING COUNT(*) = 1
) m ON m.ref_id = w.outfit_ref_id
JOIN lookbook_look l ON l.id = m.look_id AND l.user_id = w.user_id
LEFT JOIN (
  SELECT DISTINCT user_id, image_path FROM outfit_image WHERE image_path IS NOT NULL
) i ON i.user_id = l.user_id AND i.image_path = l.image_path
SET
  w.pinned_look_id = l.id,
  w.pinned_image_path = i.image_path,
  w.photo_basis = IF(i.image_path IS NULL, NULL, 'migrated'),
  w.photo_pinned_at = IF(i.image_path IS NULL, NULL, NOW()),
  w.updated_at = w.updated_at
WHERE w.pinned_look_id IS NULL
  AND (m.needs_photo = 0 OR i.image_path IS NOT NULL);
