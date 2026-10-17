use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection, QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use shipping_application::ApplicationError;
use shipping_domain::DomainError;

/// Uniform error body: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: &self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<ApplicationError> for ApiError {
    fn from(error: ApplicationError) -> Self {
        use ApplicationError as E;

        let message = error.to_string();
        match error {
            E::Domain(domain) => domain_error(&domain, message),
            E::ShipmentNotFound(_) => {
                Self::new(StatusCode::NOT_FOUND, "shipment_not_found", message)
            }
            E::NoDroneAvailable => Self::new(StatusCode::CONFLICT, "no_drone_available", message),
            E::RouteUnavailable(_) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "route_unavailable",
                message,
            ),
            E::ConcurrentModification => {
                Self::new(StatusCode::CONFLICT, "concurrent_modification", message)
            }
            E::DependencyUnavailable { .. } => {
                tracing::warn!(%message, "dependency unavailable");
                Self::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "dependency_unavailable",
                    message,
                )
            }
            E::DependencyRejected { .. } => {
                tracing::error!(%message, "dependency rejected our request");
                Self::new(StatusCode::BAD_GATEWAY, "dependency_error", message)
            }
            E::Internal(_) => {
                tracing::error!(%message, "internal error");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "an internal error occurred",
                )
            }
        }
    }
}

fn domain_error(error: &DomainError, message: String) -> ApiError {
    use DomainError as D;

    let (status, code) = match error {
        D::InvalidStateTransition { .. } => (StatusCode::CONFLICT, "invalid_state_transition"),
        D::DroneCannotCarryParcel { .. } | D::RouteExceedsDroneRange { .. } => {
            (StatusCode::CONFLICT, "drone_not_suitable")
        }
        D::InvalidCoordinates => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_coordinates"),
        D::BlankField(_) | D::FieldTooLong { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_field")
        }
        D::InvalidPhoneNumber => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_phone_number"),
        D::InvalidParcelWeight { .. } | D::InvalidParcelDimensions { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_parcel")
        }
        D::InvalidTrackingNumber => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_tracking_number"),
        D::LocationsTooClose { .. } | D::OutsideServiceArea { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "unsupported_route")
        }
        D::InvalidRoutePlan | D::NoDroneAssigned | D::UnknownStatus(_) => {
            tracing::error!(%message, "domain invariant broken by stored data");
            return ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "an internal error occurred",
            );
        }
    };
    ApiError::new(status, code, message)
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), "invalid_request", rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::new(rejection.status(), "invalid_request", rejection.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::new(rejection.status(), "invalid_request", rejection.body_text())
    }
}
