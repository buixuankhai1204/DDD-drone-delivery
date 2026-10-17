use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Lifecycle of a shipment.
///
/// ```text
/// Requested ──► Scheduled ──► InTransit ──► Delivered
///     │             │             │
///     └──► Cancelled ◄┘          └──► Failed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShipmentStatus {
    /// Accepted from the customer, waiting for a drone.
    Requested,
    /// A drone has been reserved and the route planned.
    Scheduled,
    /// The drone picked up the parcel and is flying to the dropoff point.
    InTransit,
    Delivered,
    Cancelled,
    /// The delivery attempt failed after pickup.
    Failed,
}

impl ShipmentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Scheduled => "scheduled",
            Self::InTransit => "in_transit",
            Self::Delivered => "delivered",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Delivered | Self::Cancelled | Self::Failed)
    }
}

impl fmt::Display for ShipmentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ShipmentStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "requested" => Ok(Self::Requested),
            "scheduled" => Ok(Self::Scheduled),
            "in_transit" => Ok(Self::InTransit),
            "delivered" => Ok(Self::Delivered),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            other => Err(DomainError::UnknownStatus(other.to_owned())),
        }
    }
}
