use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderName, StatusCode, header},
    response::IntoResponse,
};
use serde::Deserialize;
use shipping_application::dto::{
    CancelShipmentCommand, ListShipmentsQuery, PagedView, ReportDeliveryFailureCommand,
    RequestShipmentCommand, ShipmentView,
};
use shipping_domain::ShipmentStatus;
use uuid::Uuid;

use crate::{
    error::ApiError,
    extract::{ApiJson, ApiPath, ApiQuery},
    state::AppState,
};

type ApiResult<T> = Result<Json<T>, ApiError>;

/// `POST /api/v1/shipments` – a customer requests a delivery.
pub async fn request_shipment(
    State(state): State<AppState>,
    ApiJson(command): ApiJson<RequestShipmentCommand>,
) -> Result<impl IntoResponse, ApiError> {
    let shipment = state.request_shipment.execute(command).await?;
    let location: [(HeaderName, String); 1] = [(
        header::LOCATION,
        format!("/api/v1/shipments/{}", shipment.id),
    )];
    Ok((StatusCode::CREATED, location, Json(shipment)))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    customer_id: Option<Uuid>,
    status: Option<ShipmentStatus>,
    limit: Option<u32>,
    offset: Option<u32>,
}

/// `GET /api/v1/shipments?customer_id=&status=&limit=&offset=`
pub async fn list_shipments(
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<ListParams>,
) -> ApiResult<PagedView<ShipmentView>> {
    let page = state
        .list_shipments
        .execute(ListShipmentsQuery {
            customer_id: params.customer_id,
            status: params.status,
            limit: params.limit,
            offset: params.offset,
        })
        .await?;
    Ok(Json(page))
}

/// `GET /api/v1/shipments/{id}`
pub async fn get_shipment(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<ShipmentView> {
    Ok(Json(state.get_shipment.execute(id).await?))
}

/// `POST /api/v1/shipments/{id}/dispatch` – assign a drone and plan the flight.
pub async fn dispatch_shipment(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<ShipmentView> {
    Ok(Json(state.dispatch_shipment.execute(id).await?))
}

#[derive(Debug, Deserialize)]
pub struct CancelBody {
    reason: Option<String>,
}

/// `POST /api/v1/shipments/{id}/cancel` – body optional: `{"reason": "..."}`.
pub async fn cancel_shipment(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
    body: Bytes,
) -> ApiResult<ShipmentView> {
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        serde_json::from_slice::<CancelBody>(&body)
            .map_err(|e| ApiError::bad_request(format!("invalid JSON body: {e}")))?
            .reason
    };
    let shipment = state
        .cancel_shipment
        .execute(CancelShipmentCommand {
            shipment_id: id,
            reason,
        })
        .await?;
    Ok(Json(shipment))
}

/// `POST /api/v1/shipments/{id}/pickup` – drone collected the parcel.
pub async fn record_pickup(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<ShipmentView> {
    Ok(Json(state.record_pickup.execute(id).await?))
}

/// `POST /api/v1/shipments/{id}/delivery` – drone dropped the parcel.
pub async fn record_delivery(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<ShipmentView> {
    Ok(Json(state.record_delivery.execute(id).await?))
}

#[derive(Debug, Deserialize)]
pub struct DeliveryFailureBody {
    reason: String,
}

/// `POST /api/v1/shipments/{id}/delivery-failure` – body: `{"reason": "..."}`.
pub async fn report_delivery_failure(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<DeliveryFailureBody>,
) -> ApiResult<ShipmentView> {
    let shipment = state
        .report_delivery_failure
        .execute(ReportDeliveryFailureCommand {
            shipment_id: id,
            reason: body.reason,
        })
        .await?;
    Ok(Json(shipment))
}
