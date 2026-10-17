//! Outbound ports. Each one is implemented by an adapter in the
//! infrastructure layer (HTTP clients for other bounded contexts, a message
//! publisher for integration events).

mod events;
mod fleet;
mod routing;

pub use events::{EventPublisher, OutboxMessage, PublishError};
pub use fleet::{DroneRequirements, FleetGateway};
pub use routing::RoutePlanner;

/// Failure when talking to another bounded context.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// Network failure, timeout or a 5xx answer: the remote side may work later.
    #[error("{service} is unavailable: {reason}")]
    Unavailable {
        service: &'static str,
        reason: String,
    },

    /// The remote side understood the request and refused it (4xx).
    #[error("{service} rejected the request: {reason}")]
    Rejected {
        service: &'static str,
        reason: String,
    },
}
