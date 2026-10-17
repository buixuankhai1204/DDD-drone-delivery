use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::value_objects::{CustomerId, DroneId, ShipmentId, TrackingNumber};

/// Facts that happened to a shipment. Other contexts (Fleet, Notification,
/// Billing, ...) react to these; they are delivered through the outbox.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShipmentEvent {
    Requested {
        shipment_id: ShipmentId,
        tracking_number: TrackingNumber,
        customer_id: CustomerId,
        occurred_at: DateTime<Utc>,
    },
    Scheduled {
        shipment_id: ShipmentId,
        drone_id: DroneId,
        estimated_delivery_at: DateTime<Utc>,
        occurred_at: DateTime<Utc>,
    },
    PickedUp {
        shipment_id: ShipmentId,
        drone_id: DroneId,
        occurred_at: DateTime<Utc>,
    },
    Delivered {
        shipment_id: ShipmentId,
        drone_id: DroneId,
        occurred_at: DateTime<Utc>,
    },
    DeliveryFailed {
        shipment_id: ShipmentId,
        drone_id: DroneId,
        reason: String,
        occurred_at: DateTime<Utc>,
    },
    Cancelled {
        shipment_id: ShipmentId,
        /// Drone that has to be released, when one was already reserved.
        drone_id: Option<DroneId>,
        reason: Option<String>,
        occurred_at: DateTime<Utc>,
    },
}

impl ShipmentEvent {
    /// Stable, dotted name used as routing key by consumers.
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::Requested { .. } => "shipment.requested",
            Self::Scheduled { .. } => "shipment.scheduled",
            Self::PickedUp { .. } => "shipment.picked_up",
            Self::Delivered { .. } => "shipment.delivered",
            Self::DeliveryFailed { .. } => "shipment.delivery_failed",
            Self::Cancelled { .. } => "shipment.cancelled",
        }
    }

    pub const fn shipment_id(&self) -> ShipmentId {
        match self {
            Self::Requested { shipment_id, .. }
            | Self::Scheduled { shipment_id, .. }
            | Self::PickedUp { shipment_id, .. }
            | Self::Delivered { shipment_id, .. }
            | Self::DeliveryFailed { shipment_id, .. }
            | Self::Cancelled { shipment_id, .. } => *shipment_id,
        }
    }

    pub const fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            Self::Requested { occurred_at, .. }
            | Self::Scheduled { occurred_at, .. }
            | Self::PickedUp { occurred_at, .. }
            | Self::Delivered { occurred_at, .. }
            | Self::DeliveryFailed { occurred_at, .. }
            | Self::Cancelled { occurred_at, .. } => *occurred_at,
        }
    }
}
