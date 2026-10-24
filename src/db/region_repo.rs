use sqlx::MySqlPool;
use uuid::Uuid;

use crate::models::region::RegionSetting;

// 지역(날씨 기준 위치)은 계정별이다(`region_setting.user_id`, 마이그레이션 20261024000003).
// 다른 계정의 위치를 읽거나 지우지 않는다. 지역이 없는 계정에 다른 계정 위치를 대신 쓰지 않는다.

pub async fn get_region(
    pool: &MySqlPool,
    owner: &str,
) -> Result<Option<RegionSetting>, sqlx::Error> {
    sqlx::query_as::<_, RegionSetting>(
        "SELECT id, name, latitude, longitude, updated_at FROM region_setting \
         WHERE user_id = ? ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(owner)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_region(
    pool: &MySqlPool,
    owner: &str,
    name: &str,
    latitude: f64,
    longitude: f64,
) -> Result<RegionSetting, sqlx::Error> {
    sqlx::query("DELETE FROM region_setting WHERE user_id = ?")
        .bind(owner)
        .execute(pool)
        .await?;

    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO region_setting (id, user_id, name, latitude, longitude) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(owner)
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
