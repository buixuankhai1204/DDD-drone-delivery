use async_trait::async_trait;
use shipping_domain::{DroneId, DroneProfile, GeoPoint, ShipmentId};

use super::GatewayError;

/// What Shipping asks the Fleet context for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DroneRequirements {
    pub payload_grams: u32,
    pub pickup: GeoPoint,
}

/// Port to the **Fleet Management** bounded context.
#[async_trait]
pub trait FleetGateway: Send + Sync {
    /// Drones able to serve `requirements`, best candidate first.
    async fn find_available_drones(
        &self,
        requirements: &DroneRequirements,
        limit: usize,
    ) -> Result<Vec<DroneProfile>, GatewayError>;

    /// Reserves the drone for the shipment. Must be idempotent for the same
    /// `(drone, shipment)` pair. Returns [`GatewayError::Rejected`] when the
    /// drone was taken in the meantime.
    async fn reserve_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError>;

    /// Gives the drone back. Idempotent: releasing an unknown reservation
    /// succeeds.
    async fn release_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError>;
}
