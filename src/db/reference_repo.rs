use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::reference::ClothingReference;

/// 모든 조회가 같은 컬럼 목록을 쓰게 한다. `SELECT *` 를 쓰지 않는 이유는
/// `FromRow` 가 컬럼 순서에 의존하지 않더라도, 컬럼이 추가될 때 어디를 고쳐야
/// 하는지 한 곳으로 모아 두는 쪽이 낫기 때문이다.
const SELECT_COLS: &str = "id, name, category, subcategory, era, style, description, \
     embedding_text, embedding, review_status, source_note, version, created_at, updated_at";

/// 새로 작성한 레퍼런스. 검수 상태를 호출자가 명시한다 — 기본값에 의존하지 않는다.
pub struct NewReference<'a> {
    pub name: &'a str,
    pub category: Option<&'a str>,
    pub subcategory: Option<&'a str>,
    pub era: Option<&'a str>,
    pub style: Option<&'a str>,
    pub description: &'a str,
    pub embedding_text: Option<&'a str>,
    pub review_status: &'a str,
    pub source_note: Option<&'a str>,
    pub genres: &'a [String],
}

pub async fn insert_reference(
    pool: &MySqlPool,
    name: &str,
    era: Option<&str>,
    style: Option<&str>,
    description: &str,
    embedding: Option<&serde_json::Value>,
) -> Result<ClothingReference, sqlx::Error> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO clothing_reference (id, name, era, style, description, embedding) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(era)
    .bind(style)
    .bind(description)
    .bind(embedding)
    .execute(pool)
    .await?;

    get_reference_by_id(pool, &id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// 이름을 키로 넣거나 갱신한다. 같은 레퍼런스를 두 번 넣어도 한 행이다.
///
/// `insert_reference` 는 항상 새 UUID 를 만들어서 중복 방지가 없었다. 같은
/// 레퍼런스가 두 행이면 검색이 둘 다 상위에 올려 참고자료 5칸 중 2칸을 같은
/// 내용으로 채운다 — 오류는 아니지만 Pass 2 가 받는 정보량이 줄어든다.
///
/// 설명이 바뀌면 `embedding` 을 NULL 로 되돌리고 `version` 을 올린다. 그래야
/// `load_cache` 가 다음 기동에 다시 임베딩한다. 설명을 고쳤는데 옛 벡터가 남아
/// 있으면 검색은 고치기 전 문장으로 이뤄지고, 그 사실은 어디에도 드러나지 않는다.
pub async fn upsert_reference(
    pool: &MySqlPool,
    r: &NewReference<'_>,
) -> Result<ClothingReference, sqlx::Error> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO clothing_reference
            (id, name, category, subcategory, era, style, description,
             embedding_text, review_status, source_note, embedding, version)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 1)
        ON DUPLICATE KEY UPDATE
            category       = VALUES(category),
            subcategory    = VALUES(subcategory),
            era            = VALUES(era),
            style          = VALUES(style),
            review_status  = VALUES(review_status),
            source_note    = VALUES(source_note),
            -- 설명이나 검색 문장이 달라졌을 때만 벡터를 버리고 버전을 올린다.
            version = version + IF(
                description <=> VALUES(description) AND embedding_text <=> VALUES(embedding_text),
                0, 1
            ),
            embedding = IF(
                description <=> VALUES(description) AND embedding_text <=> VALUES(embedding_text),
                embedding, NULL
            ),
            description    = VALUES(description),
            embedding_text = VALUES(embedding_text),
            updated_at     = NOW()
        "#,
    )
    .bind(&id)
    .bind(r.name)
    .bind(r.category)
    .bind(r.subcategory)
    .bind(r.era)
    .bind(r.style)
    .bind(r.description)
    .bind(r.embedding_text)
    .bind(r.review_status)
    .bind(r.source_note)
    .execute(pool)
    .await?;

    let saved = get_reference_by_name(pool, r.name)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    // 장르는 전량 교체한다. 빼야 할 장르가 남아 있으면 커버리지 통계가 틀어진다.
    sqlx::query("DELETE FROM clothing_reference_genre WHERE reference_id = ?")
        .bind(&saved.id)
        .execute(pool)
        .await?;
    for genre in r.genres {
        sqlx::query(
            "INSERT IGNORE INTO clothing_reference_genre (reference_id, style_genre) VALUES (?, ?)",
        )
        .bind(&saved.id)
        .bind(genre)
        .execute(pool)
        .await?;
    }

    Ok(saved)
}

pub async fn list_references(pool: &MySqlPool) -> Result<Vec<ClothingReference>, sqlx::Error> {
    sqlx::query_as::<_, ClothingReference>(&format!(
        "SELECT {SELECT_COLS} FROM clothing_reference ORDER BY name"
    ))
    .fetch_all(pool)
    .await
}

pub async fn get_reference_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<ClothingReference>, sqlx::Error> {
    sqlx::query_as::<_, ClothingReference>(&format!(
        "SELECT {SELECT_COLS} FROM clothing_reference WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_reference_by_name(
    pool: &MySqlPool,
    name: &str,
) -> Result<Option<ClothingReference>, sqlx::Error> {
    sqlx::query_as::<_, ClothingReference>(&format!(
        "SELECT {SELECT_COLS} FROM clothing_reference WHERE name = ?"
    ))
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub async fn update_reference(
    pool: &MySqlPool,
    id: &str,
    name: Option<&str>,
    era: Option<&str>,
    style: Option<&str>,
    description: Option<&str>,
    embedding: Option<&serde_json::Value>,
) -> Result<Option<ClothingReference>, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE clothing_reference SET
            name        = COALESCE(?, name),
            era         = COALESCE(?, era),
            style       = COALESCE(?, style),
            description = COALESCE(?, description),
            embedding   = COALESCE(?, embedding),
            updated_at  = NOW()
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(era)
    .bind(style)
    .bind(description)
    .bind(embedding)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    get_reference_by_id(pool, id).await
}

pub async fn delete_reference(pool: &MySqlPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM clothing_reference WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn count_references(pool: &MySqlPool) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clothing_reference")
        .fetch_one(pool)
        .await?;
    Ok(count)
}
