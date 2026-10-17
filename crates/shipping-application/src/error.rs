use shipping_domain::{DomainError, RepositoryError};

use crate::ports::GatewayError;

#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("shipment '{0}' was not found")]
    ShipmentNotFound(String),

    #[error("no suitable drone is available right now")]
    NoDroneAvailable,

    #[error("no route available: {0}")]
    RouteUnavailable(String),

    #[error("{service} is unavailable: {reason}")]
    DependencyUnavailable {
        service: &'static str,
        reason: String,
    },

    #[error("{service} rejected the request: {reason}")]
    DependencyRejected {
        service: &'static str,
        reason: String,
    },

    #[error("the shipment was modified concurrently, please retry")]
    ConcurrentModification,

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<RepositoryError> for ApplicationError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(_) => Self::ConcurrentModification,
            RepositoryError::Storage(source) => Self::Internal(source.to_string()),
        }
    }
}

impl From<GatewayError> for ApplicationError {
    fn from(error: GatewayError) -> Self {
        match error {
            GatewayError::Unavailable { service, reason } => {
                Self::DependencyUnavailable { service, reason }
            }
            GatewayError::Rejected { service, reason } => {
                Self::DependencyRejected { service, reason }
            }
        }
    }
}
