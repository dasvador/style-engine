-- 스타일 메타데이터 어휘를 표준값으로 정규화하고, DB 차원에서 강제한다.
--
-- 배경: 2026-05 여성 아이템 시드가 엔진과 다른 어휘로 삽입되었다. `role`에는 영어
-- (base/accent/outer), `style`에는 무드 분류(boho/office/street...)가 들어갔다.
-- 엔진은 문자열을 그대로 비교하므로 281건 중 141건이 역할 기반 규칙에 투명했다.
-- 앞선 20260811000001(밥/반찬 → 베이스/포인트)은 이 영어 값들을 건드리지 않았다.
--
-- 이 마이그레이션 이후 표준 어휘는 코드에서 타입(models/style_vocab.rs)으로,
-- DB에서 CHECK 제약으로 강제된다. 어느 쪽으로 잘못된 값이 들어와도 조용히 무시되지 않는다.

-- ─── role ───
-- base/accent 는 표준 역할에 1:1 대응한다.
UPDATE clothing SET role = '베이스' WHERE role = 'base';
UPDATE clothing SET role = '포인트' WHERE role = 'accent';

-- 'outer' 24건은 전부 category='아우터' 이다. 역할이 아니라 카테고리가 잘못 들어간 값이라
-- 대응되는 표준 역할이 없다. 아우터의 역할은 아이템마다 다르므로(포인트/구조템/연결템)
-- 임의로 채우면 바로잡으려던 신호를 다시 오염시킨다. NULL로 두고 재라벨링 대상으로 남긴다.
UPDATE clothing SET role = NULL WHERE role = 'outer';

-- ─── tone ───
-- 활용형 차이. 의미는 동일하다.
UPDATE clothing SET tone = '밝음'   WHERE tone = '밝은';
UPDATE clothing SET tone = '어두움' WHERE tone = '어두운';

-- ─── style ───
-- style 은 '스타일 충돌' 판정용 축(베이직/워크/밀리터리/포멀/스포츠)이고,
-- 무드는 별도 컬럼(style_mood)에 이미 들어가 있다. 여성 행의 style 에는 무드 어휘가
-- 잘못 들어갔으므로, 충돌 축에 정직하게 대응되는 것만 옮기고 나머지는 비운다.
UPDATE clothing SET style = '밀리터리' WHERE style = 'military';
UPDATE clothing SET style = '워크'     WHERE style = 'workwear';
UPDATE clothing SET style = '스포츠'   WHERE style = 'sporty';
UPDATE clothing SET style = '포멀'     WHERE style = 'office';
UPDATE clothing SET style = '베이직'   WHERE style IN ('minimal', 'casual');

-- boho / romantic / feminine / street / vintage 는 충돌 축에 대응값이 없다.
-- '베이직'으로 채우면 일부 규칙에서 "스타일 일치" 보너스를 부당하게 얻는다.
-- 신호 없음(NULL)이 정직하다. 원래 값은 style_mood 에 그대로 보존되어 있다.
UPDATE clothing
SET style = NULL
WHERE style IN ('boho', 'romantic', 'feminine', 'street', 'vintage');

-- ─── DB 차원 강제 ───
-- 코드가 타입으로 막더라도, 수동 INSERT·시드 스크립트·다른 클라이언트는 우회할 수 있다.
-- 표준 밖의 값은 저장 자체가 실패해야 한다. (NULL 은 CHECK 를 통과한다 = '값 없음' 허용)

ALTER TABLE clothing
    ADD CONSTRAINT chk_clothing_role
        CHECK (role IS NULL OR role IN ('베이스','포인트','약한포인트','연결템','구조템')),
    ADD CONSTRAINT chk_clothing_tone
        CHECK (tone IS NULL OR tone IN ('밝음','중간','어두움')),
    ADD CONSTRAINT chk_clothing_saturation
        CHECK (saturation IS NULL OR saturation IN ('낮음','중간','높음')),
    ADD CONSTRAINT chk_clothing_style
        CHECK (style IS NULL OR style IN ('베이직','워크','밀리터리','포멀','스포츠')),
    ADD CONSTRAINT chk_clothing_weight
        CHECK (weight IS NULL OR weight IN ('가벼움','중간','무거움'));
