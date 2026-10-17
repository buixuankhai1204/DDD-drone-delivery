use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use shipping_application::ports::{EventPublisher, OutboxMessage};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DEFAULT_BATCH_SIZE: i64 = 50;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Moves events from the `outbox_events` table to an [`EventPublisher`].
///
/// Delivery is **at-least-once** and ordered by insertion: an event is only
/// marked as published after the publisher accepted it, and a failing event
/// blocks the ones behind it until it succeeds. Consumers must therefore
/// de-duplicate on [`OutboxMessage::id`].
pub struct OutboxRelay {
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
    batch_size: i64,
    poll_interval: Duration,
}

#[derive(sqlx::FromRow)]
struct OutboxRow {
    seq: i64,
    id: Uuid,
    aggregate_id: Uuid,
    event_type: String,
    payload: serde_json::Value,
    occurred_at: DateTime<Utc>,
}

impl OutboxRelay {
    pub fn new(pool: PgPool, publisher: Arc<dyn EventPublisher>) -> Self {
        Self {
            pool,
            publisher,
            batch_size: DEFAULT_BATCH_SIZE,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    /// Polls until `shutdown` is cancelled.
    pub async fn run(self, shutdown: CancellationToken) {
        tracing::info!("outbox relay started");
        loop {
            match self.relay_batch().await {
                // A full batch means there is probably more waiting.
                Ok(published) if published as i64 >= self.batch_size => continue,
                Ok(_) => {}
                Err(error) => tracing::error!(%error, "outbox relay failed"),
            }

            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = tokio::time::sleep(self.poll_interval) => {}
            }
        }
        tracing::info!("outbox relay stopped");
    }

    /// Publishes one batch of pending events and returns how many succeeded.
    pub async fn relay_batch(&self) -> Result<usize, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Row locks keep several relay instances from publishing the same
        // events at once; the others wait and then see them as published.
        let rows = sqlx::query_as::<_, OutboxRow>(
            "SELECT seq, id, aggregate_id, event_type, payload, occurred_at
             FROM outbox_events
             WHERE published_at IS NULL
             ORDER BY seq
             LIMIT $1
             FOR UPDATE",
        )
        .bind(self.batch_size)
        .fetch_all(&mut *tx)
        .await?;

        let mut published = 0;
        for row in rows {
            let message = OutboxMessage {
                id: row.id,
                aggregate_id: row.aggregate_id,
                event_type: row.event_type,
                payload: row.payload,
                occurred_at: row.occurred_at,
            };
            if let Err(error) = self.publisher.publish(&message).await {
                tracing::warn!(event_id = %message.id, %error, "publishing failed, will retry");
                break;
            }

            sqlx::query("UPDATE outbox_events SET published_at = now() WHERE seq = $1")
                .bind(row.seq)
                .execute(&mut *tx)
                .await?;
            published += 1;
        }

        tx.commit().await?;
        Ok(published)
    }
}
