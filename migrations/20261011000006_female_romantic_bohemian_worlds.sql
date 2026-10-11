-- 월드가 없던 여성 의류 33벌 중 30벌에 텍스처 월드를 붙인다 (2026-10-11 검수, 제안대로 승인).
--
-- 20261007000002 는 남성 배정에서 역산한 규칙으로 채웠고, 두 부류가 남았다.
--   - 로맨틱·보헤미안 18벌: 새틴·쉬폰·오간자·레이스·자수·셔링·크로셰·티어드. 맞는 월드가
--     없어서 `romantic`, `bohemian` 을 새로 만든다.
--   - 스타일 값이 비어 규칙에서 빠진 기본템·정장류 12벌: 같은 품목의 기존 배정을 따른다
--     (여성 트렌치코트 2벌이 minimal, 오버핏 면 셔츠가 minimal).
-- 스트리트 3벌(그래픽 티, 레터링 크롭 탑, 메쉬 롱슬리브)은 맞는 월드가 없어 비워 둔다.
-- 스포츠 옷은 남성과 같이 일부러 비워 둔다.
--
-- 새 월드를 읽는 규칙은 아직 없다. 로맨틱·보헤미안 옷과 다른 월드의 조합 16개는 모두
-- "보완" 으로 판단받았고(tests/fixtures/recommendation_cases.toml 의 RW01~RW16), 규칙은
-- 그 라벨로 엔진 판정을 확인한 뒤에 정한다.
--
-- 이름으로 고르는 이유: 이 30벌은 품목·소재로 묶이지 않는다(같은 cotton 블라우스가
-- romantic 과 minimal 로 갈린다). 사람이 한 벌씩 본 결정이라 그대로 적는다.
-- 재실행해도 결과가 같다 (INSERT IGNORE + 복합 PK).

-- romantic 12벌
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT id, 'romantic' FROM clothing
WHERE gender = 'female' AND name IN (
    '화이트 코튼 스모킹 크롭 탑',
    '라벤더 리본 넥 퍼프슬리브 블라우스',
    '아이보리 플로럴 자수 오간자 블라우스',
    '크림 스모크 셔링 탑',
    '화이트 퍼프슬리브 티셔츠',
    '화이트 레이스 트리밍 쉬폰 블라우스',
    '크림 레이스 패치 니트 카디건',
    '크림 플리츠 미디스커트',
    '베이비핑크 플리츠 새틴 미디 스커트',
    '핑크 플로럴 프린트 랩 미디 스커트',
    '아이보리 새틴 셔츠 블라우스',
    '아이보리 V넥 실크터치 블라우스'
);

-- bohemian 6벌
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT id, 'bohemian' FROM clothing
WHERE gender = 'female' AND name IN (
    '크림 오프숄더 보헤미안 블라우스',
    '테라코타 크링클 코튼 탑',
    '아이보리 에스닉 자수 블라우스',
    '크림 크로셰 니트 가디건',
    '머스타드 프린트 티어드 맥시스커트',
    '베이지 플로럴 프린트 맥시 원피스'
);

-- tailoring 3벌
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT id, 'tailoring' FROM clothing
WHERE gender = 'female' AND name IN (
    '아이보리 트위드 자켓',
    '라이트핑크 울혼방 숏 자켓',
    '블랙 H라인 미디 스커트'
);

-- minimal 9벌
INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world)
SELECT id, 'minimal' FROM clothing
WHERE gender = 'female' AND name IN (
    '아이보리 코튼 블라우스',
    '라벤더 리브드 니트',
    '베이지 크롭 가디건',
    '그레이 니트 터틀넥 탑',
    '화이트 코튼 오버사이즈 셔츠',
    '라이트베이지 와이드 코튼팬츠',
    '화이트 A라인 미니스커트',
    '민트 체크 패턴 트렌치 코트',
    '라이트핑크 코튼 트렌치코트'
);
