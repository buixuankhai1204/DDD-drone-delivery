use axum::{Json, extract::State};
use shipping_application::dto::TrackingView;

use crate::{error::ApiError, extract::ApiPath, state::AppState};

/// `GET /api/v1/tracking/{tracking_number}` – public, personal data excluded.
pub async fn track_shipment(
    State(state): State<AppState>,
    ApiPath(tracking_number): ApiPath<String>,
) -> Result<Json<TrackingView>, ApiError> {
    Ok(Json(state.track_shipment.execute(&tracking_number).await?))
}
