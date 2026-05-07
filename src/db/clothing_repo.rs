use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::clothing::Clothing;

const SELECT_COLS: &str = "id, name, category, color, thickness, image_url, tone, saturation, style, weight, role, color_temperature, versatility, statement_level, formality_level, visual_weight, texture_depth, visual_weight_v2, texture_depth_v2, grounding_score, shadow_tone, silhouette_volume, material_primary, sub_category, floating_score, strong_style_score, texture_keywords, created_at, updated_at";

pub async fn insert_clothing(
    pool: &MySqlPool,
    name: &str,
    category: &str,
    color: Option<&str>,
    thickness: &str,
    image_url: Option<&str>,
    tone: Option<&str>,
    saturation: Option<&str>,
    style: Option<&str>,
    weight: Option<&str>,
    role: Option<&str>,
    color_temperature: Option<&str>,
    versatility: Option<&str>,
    statement_level: Option<i8>,
    formality_level: Option<i8>,
) -> Result<Clothing, sqlx::Error> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO clothing (id, name, category, color, thickness, image_url, tone, saturation, style, weight, role, color_temperature, versatility, statement_level, formality_level) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(category)
    .bind(color)
    .bind(thickness)
    .bind(image_url)
    .bind(tone)
    .bind(saturation)
    .bind(style)
    .bind(weight)
    .bind(role)
    .bind(color_temperature)
    .bind(versatility)
    .bind(statement_level)
    .bind(formality_level)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Clothing>(&format!("SELECT {} FROM clothing WHERE id = ?", SELECT_COLS))
        .bind(&id)
        .fetch_one(pool)
        .await
}

pub async fn insert_seasons(
    pool: &MySqlPool,
    clothing_id: &str,
    seasons: &[String],
) -> Result<(), sqlx::Error> {
    for season in seasons {
        sqlx::query("INSERT IGNORE INTO clothing_season (clothing_id, season) VALUES (?, ?)")
            .bind(clothing_id)
            .bind(season)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_seasons(pool: &MySqlPool, clothing_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT season FROM clothing_season WHERE clothing_id = ?")
            .bind(clothing_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn insert_texture_worlds(
    pool: &MySqlPool,
    clothing_id: &str,
    worlds: &[String],
) -> Result<(), sqlx::Error> {
    for w in worlds {
        sqlx::query(
            "INSERT IGNORE INTO clothing_texture_world (clothing_id, texture_world) VALUES (?, ?)",
        )
        .bind(clothing_id)
        .bind(w)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_texture_worlds(
    pool: &MySqlPool,
    clothing_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT texture_world FROM clothing_texture_world WHERE clothing_id = ?")
            .bind(clothing_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn delete_texture_worlds(
    pool: &MySqlPool,
    clothing_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM clothing_texture_world WHERE clothing_id = ?")
        .bind(clothing_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_clothing(pool: &MySqlPool) -> Result<Vec<Clothing>, sqlx::Error> {
    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing ORDER BY created_at DESC",
        SELECT_COLS
    ))
    .fetch_all(pool)
    .await
}

pub async fn get_clothing_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<Clothing>, sqlx::Error> {
    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing WHERE id = ?",
        SELECT_COLS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn update_clothing(
    pool: &MySqlPool,
    id: &str,
    name: Option<&str>,
    category: Option<&str>,
    color: Option<&str>,
    thickness: Option<&str>,
    image_url: Option<&str>,
    tone: Option<&str>,
    saturation: Option<&str>,
    style: Option<&str>,
    weight: Option<&str>,
    role: Option<&str>,
    color_temperature: Option<&str>,
    versatility: Option<&str>,
    statement_level: Option<i8>,
    formality_level: Option<i8>,
) -> Result<Option<Clothing>, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE clothing SET
            name              = COALESCE(?, name),
            category          = COALESCE(?, category),
            color             = COALESCE(?, color),
            thickness         = COALESCE(?, thickness),
            image_url         = COALESCE(?, image_url),
            tone              = COALESCE(?, tone),
            saturation        = COALESCE(?, saturation),
            style             = COALESCE(?, style),
            weight            = COALESCE(?, weight),
            role              = COALESCE(?, role),
            color_temperature = COALESCE(?, color_temperature),
            versatility       = COALESCE(?, versatility),
            statement_level   = COALESCE(?, statement_level),
            formality_level   = COALESCE(?, formality_level),
            updated_at        = NOW()
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(category)
    .bind(color)
    .bind(thickness)
    .bind(image_url)
    .bind(tone)
    .bind(saturation)
    .bind(style)
    .bind(weight)
    .bind(role)
    .bind(color_temperature)
    .bind(versatility)
    .bind(statement_level)
    .bind(formality_level)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    sqlx::query_as::<_, Clothing>(&format!(
        "SELECT {} FROM clothing WHERE id = ?",
        SELECT_COLS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn delete_seasons(pool: &MySqlPool, clothing_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM clothing_season WHERE clothing_id = ?")
        .bind(clothing_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_clothing(pool: &MySqlPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM clothing WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
