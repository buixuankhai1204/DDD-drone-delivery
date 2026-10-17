mod health;
mod shipments;
mod tracking;

use std::time::Duration;

use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

use crate::{error::ApiError, state::AppState};

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route(
            "/shipments",
            post(shipments::request_shipment).get(shipments::list_shipments),
        )
        .route("/shipments/{id}", get(shipments::get_shipment))
        .route(
            "/shipments/{id}/dispatch",
            post(shipments::dispatch_shipment),
        )
        .route("/shipments/{id}/cancel", post(shipments::cancel_shipment))
        .route("/shipments/{id}/pickup", post(shipments::record_pickup))
        .route("/shipments/{id}/delivery", post(shipments::record_delivery))
        .route(
            "/shipments/{id}/delivery-failure",
            post(shipments::report_delivery_failure),
        )
        .route("/tracking/{tracking_number}", get(tracking::track_shipment));

    Router::new()
        .nest("/api/v1", api)
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "route not found") })
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
