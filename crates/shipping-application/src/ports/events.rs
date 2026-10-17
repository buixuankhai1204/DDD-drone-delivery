use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A domain event as stored in the transactional outbox.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxMessage {
    /// Unique per event; consumers can use it to de-duplicate (delivery is
    /// at-least-once).
    pub id: Uuid,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
#[error("failed to publish event: {0}")]
pub struct PublishError(pub String);

/// Port that ships integration events to the rest of the system (message
/// broker, webhooks, ...).
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, message: &OutboxMessage) -> Result<(), PublishError>;
}
