use chrono::{DateTime, Utc};

use crate::value_objects::DroneId;

/// What the Shipping context needs to know about a drone.
///
/// Deliberately tiny: this is Shipping's own model of a drone, translated from
/// whatever the Fleet context exposes (anti-corruption layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DroneProfile {
    pub id: DroneId,
    pub max_payload_grams: u32,
    /// Usable flight distance for one mission, return-to-base reserve excluded.
    pub max_range_m: u32,
}

/// Link between a shipment and the drone that will carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DroneAssignment {
    pub drone_id: DroneId,
    pub assigned_at: DateTime<Utc>,
}
