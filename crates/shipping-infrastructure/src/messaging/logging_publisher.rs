use async_trait::async_trait;
use shipping_application::ports::{EventPublisher, OutboxMessage, PublishError};

/// Publisher that only logs events. Placeholder until a message broker
/// (Kafka, NATS, RabbitMQ, ...) is chosen: swap it for an adapter that
/// implements [`EventPublisher`] against that broker.
#[derive(Default)]
pub struct LoggingEventPublisher;

#[async_trait]
impl EventPublisher for LoggingEventPublisher {
    async fn publish(&self, message: &OutboxMessage) -> Result<(), PublishError> {
        tracing::info!(
            event_id = %message.id,
            event_type = %message.event_type,
            aggregate_id = %message.aggregate_id,
            payload = %message.payload,
            "integration event published"
        );
        Ok(())
    }
}
