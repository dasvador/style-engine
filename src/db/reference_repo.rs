use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::reference::ClothingReference;

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

pub async fn list_references(pool: &MySqlPool) -> Result<Vec<ClothingReference>, sqlx::Error> {
    sqlx::query_as::<_, ClothingReference>(
        "SELECT id, name, era, style, description, embedding, created_at, updated_at FROM clothing_reference ORDER BY name",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_reference_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<ClothingReference>, sqlx::Error> {
    sqlx::query_as::<_, ClothingReference>(
        "SELECT id, name, era, style, description, embedding, created_at, updated_at FROM clothing_reference WHERE id = ?",
    )
    .bind(id)
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
