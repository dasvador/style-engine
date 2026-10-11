-- 역할이 비어 있던 여성 아우터 24벌의 역할을 채운다 (2026-10-11 검수, tools/world-review/outer_roles.toml).
--
-- 기준은 남성 데이터다: 형태가 잡힌 블레이저·코트·트렌치·봄버·짙은 데님 → 구조템,
-- 가볍게 걸치는 데님·코듀로이·필드자켓·후드집업 → 연결템, 색·무늬·질감이 튀는 아우터
-- → 약한포인트. 라이트핑크 울혼방 숏 자켓은 사용자가 포인트로 골랐다.
--
-- 역할이 비어 있으면 운영 엔진은 구조템·연결템과 똑같이 보므로 그 18벌은 점수가 그대로다.
-- 바뀌는 것은 약한포인트 5벌(그 옷이 든 착장 평균 약 -5점)과 포인트 1벌(평균 약 -22점,
-- 다른 포인트와 겹쳐 감점). 측정: 이 아우터들이 든 착장 43,776개 중 9,436개 점수가
-- 바뀌고 전체 평균 -2.0점.
--
-- role IS NULL 일 때만 채워서, 그 사이 사람이 정한 값은 덮지 않는다.

UPDATE clothing SET role = '구조템'
WHERE gender = 'female' AND category = '아우터' AND role IS NULL AND name IN (
    '워시드 블랙 크롭 데님자켓',
    '크림 숏 트렌치코트',
    '블랙 싱글 테일러드 자켓',
    '크림 울혼방 롱 코트',
    '아이보리 트위드 자켓',
    '카멜 싱글 버튼 테일러드 자켓',
    '라이트베이지 오버핏 블레이저',
    '네이비 더블 브레스티드 블레이저',
    '블랙 나일론 윈드브레이커',
    '블랙 나일론 봄버자켓',
    '베이지 오버사이즈 울 코트',
    '네이비 코튼 트렌치코트'
);

UPDATE clothing SET role = '연결템'
WHERE gender = 'female' AND category = '아우터' AND role IS NULL AND name IN (
    '올리브 오버사이즈 린넨 셔츠자켓',
    '올리브 코튼 필드자켓',
    '워시드 라이트블루 데님자켓',
    '베이지 코듀로이 트러커자켓',
    '라이트그레이 코튼 크롭 후드집업',
    '올리브 오버핏 윈드브레이커'
);

UPDATE clothing SET role = '약한포인트'
WHERE gender = 'female' AND category = '아우터' AND role IS NULL AND name IN (
    '크림 크로셰 니트 가디건',
    '라이트핑크 코튼 트렌치코트',
    '탄 스웨이드 프린지 크롭 자켓',
    '민트 체크 패턴 트렌치 코트',
    '크림 메쉬 패널 트랙재킷'
);

UPDATE clothing SET role = '포인트'
WHERE gender = 'female' AND category = '아우터' AND role IS NULL AND name IN (
    '라이트핑크 울혼방 숏 자켓'
);
