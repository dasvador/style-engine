use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::region::RegionSetting;

pub async fn get_region(pool: &MySqlPool) -> Result<Option<RegionSetting>, sqlx::Error> {
    sqlx::query_as::<_, RegionSetting>(
        "SELECT id, name, latitude, longitude, updated_at FROM region_setting ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

pub async fn upsert_region(
    pool: &MySqlPool,
    name: &str,
    latitude: f64,
    longitude: f64,
) -> Result<RegionSetting, sqlx::Error> {
    sqlx::query("DELETE FROM region_setting")
        .execute(pool)
        .await?;

    let id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO region_setting (id, name, latitude, longitude) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(latitude)
        .bind(longitude)
        .execute(pool)
        .await?;

    sqlx::query_as::<_, RegionSetting>(
        "SELECT id, name, latitude, longitude, updated_at FROM region_setting WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}
