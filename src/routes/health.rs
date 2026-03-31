use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(health_check))
}

async fn health_check(State(state): State<AppState>) -> Json<Value> {
    // Quick DB ping to verify connectivity
    let db_ok = sqlx::query("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    Json(json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "database": db_ok,
    }))
}
