-- 레퍼런스 지식 베이스를 검색 가능한 구조로 확장한다.
--
-- ─── 왜 필요한가 (실측) ───
--
-- 현재 스키마는 `id, name, era, style, description, embedding` 뿐이다. 카테고리가
-- 없어서 검색이 의류 종류를 구분하지 못한다. 2026-10-11 에 DB 의 13개 레퍼런스
-- 임베딩으로 측정한 결과:
--
--   정답이 KB 에 있는 질의 4건  → top1 유사도 0.378 ~ 0.579 (Recall@1 = 4/4)
--   정답이 KB 에 없는 질의 6건  → top1 유사도 0.247 ~ 0.527
--
-- 두 분포가 겹치므로 `prompts.rs` 의 전역 컷오프 0.5 로는 분리되지 않는다. 실제로
-- M-65 질의(0.442)는 정답을 1순위로 찾아 놓고 폴백으로 버려지고, 플리츠 미디 스커트
-- 질의(0.527)는 통과해서 `셀비지 데님 진` 설명을 Pass 2 의 참고자료로 받는다.
--
-- 오답 6건의 공통점은 전부 **의류 카테고리가 다른** 레퍼런스에 걸렸다는 것이다
-- (블라우스·스커트·가디건·힐·가방 → 청바지/스웻셔츠). 정답 4건은 모두 같은
-- 카테고리끼리 걸렸다. 그래서 임계값을 올리고 내리는 대신 카테고리로 먼저 거른다.
--
-- ─── 컬럼 ───
--
--   category      — `clothing.category` 와 같은 어휘를 쓴다(상의/하의/아우터/신발/
--                   가방/액세서리/모자/벨트). Vision Pass 1 이 판단한 카테고리와
--                   직접 비교해야 하므로 새 어휘를 만들지 않는다.
--   subcategory   — `clothing.sub_category` 와 같은 축. 선택.
--   embedding_text— 검색용 축약 표현. NULL 이면 `description` 을 그대로 쓴다
--                   (기존 13행 호환). 왜 분리하나: 현재 설명에는 "주요 브랜드:
--                   Warehouse, Levi's Vintage Clothing(LVC), Fullcount..." 같은
--                   문장이 시각적 특징과 같은 벡터에 들어가 있다. 검색 적합성은
--                   관찰 가능한 형태·소재·디테일로 판단해야 한다.
--   review_status — 'approved' | 'draft'. 업로드 분석 경로는 approved 만 검색한다.
--                   생성형 AI 가 쓴 설명은 틀려도 코드가 오류를 내지 않는다 — 그대로
--                   Pass 2 프롬프트에 들어가 Vision 의 판단 근거가 되고, 결과는 그냥
--                   조금 더 틀린 분석으로 나온다. 그래서 승인 단계를 둔다.
--   source_note   — 작성 근거. 어떤 자료를 보고 썼는지, 무엇이 추정인지.
--   version       — 설명을 고칠 때 올린다. 임베딩 재생성 판단에 쓴다.
--
-- DEFAULT 를 'approved' 로 두는 이유: 이 마이그레이션이 도는 시점에 존재하는 행은
-- 기존 시드 13개뿐이고, 그것들은 지금 운영에서 검색되고 있다. 기본값을 'draft' 로
-- 두면 UPDATE 문이 필요해지고, 그 UPDATE 를 나중에 다시 실행하면 검수 대기 중인
-- 데이터까지 승인된다. 기본값으로 처리해서 이 마이그레이션을 순수 DDL 로 남긴다.
-- 새로 넣는 레퍼런스는 `reference_repo::upsert_reference` 가 상태를 명시해 넣는다.
--
-- `name` 에 UNIQUE 를 거는 이유: `insert_reference` 가 항상 새 UUID 를 만들어서
-- 이름 기준 중복 방지가 없었다. 재시딩하면 같은 레퍼런스가 두 행으로 쌓이고, 검색은
-- 둘 다 상위에 올려 참고자료 5칸 중 2칸을 같은 내용으로 채운다. 현재 13행의 이름은
-- 모두 다르므로 제약을 걸 수 있다.

ALTER TABLE clothing_reference
  ADD COLUMN category      VARCHAR(20)  DEFAULT NULL COMMENT '상의/하의/아우터/신발/가방/액세서리/모자/벨트' AFTER name,
  ADD COLUMN subcategory   VARCHAR(50)  DEFAULT NULL COMMENT 'clothing.sub_category 와 같은 축' AFTER category,
  ADD COLUMN embedding_text TEXT        DEFAULT NULL COMMENT '검색용 축약 표현. NULL 이면 description 사용' AFTER description,
  ADD COLUMN review_status  VARCHAR(16) NOT NULL DEFAULT 'approved' COMMENT 'approved | draft',
  ADD COLUMN source_note    TEXT        DEFAULT NULL COMMENT '작성 근거와 추정 표시',
  ADD COLUMN version        INT         NOT NULL DEFAULT 1,
  ADD UNIQUE KEY uk_clothing_reference_name (name);

CREATE INDEX idx_clothing_reference_lookup
  ON clothing_reference (review_status, category);

-- 장르는 다대다다. 로퍼는 클래식이면서 프레피이고 미니멀이다. 기존 `style` 컬럼은
-- `밀리터리/워크웨어` 처럼 한 칸에 두 개를 적는 자유 문자열이라 조회에 쓸 수 없고,
-- 호환을 위해 그대로 둔다.
--
-- `style_genre` 값은 `StyleGenre` 어휘와 같아야 한다 — minimal, classic, romantic,
-- modern_chic, bohemian, street, mannish, sporty_casual, amekaji, preppy, workwear,
-- outdoor_casual. 성별별로 8개가 노출된다(공용 4 + 성별 전용 4, `style_mood` 표).
CREATE TABLE IF NOT EXISTS clothing_reference_genre (
    reference_id CHAR(36)    NOT NULL,
    style_genre  VARCHAR(50) NOT NULL,
    PRIMARY KEY (reference_id, style_genre),
    CONSTRAINT fk_reference_genre_reference
        FOREIGN KEY (reference_id) REFERENCES clothing_reference (id)
        ON DELETE CASCADE
);
