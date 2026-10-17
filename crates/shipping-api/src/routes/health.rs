use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};
use shipping_infrastructure::database;

use crate::state::AppState;

/// The process is up.
pub async fn live() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// The process can serve traffic: the database answers.
pub async fn ready(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match database::ping(&state.db).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(error) => {
            tracing::warn!(%error, "readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "unavailable", "reason": "database unreachable" })),
            )
        }
    }
}
