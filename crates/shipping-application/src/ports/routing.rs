use async_trait::async_trait;
use shipping_domain::{DroneId, GeoPoint, RoutePlan};

use super::GatewayError;

/// Port to the **Flight Planning** bounded context.
#[async_trait]
pub trait RoutePlanner: Send + Sync {
    /// Plans the flight of `drone_id` from its current position to `pickup`
    /// and on to `dropoff`.
    ///
    /// [`GatewayError::Rejected`] means no flyable route exists (restricted
    /// airspace, weather, ...).
    async fn plan_route(
        &self,
        drone_id: DroneId,
        pickup: &GeoPoint,
        dropoff: &GeoPoint,
    ) -> Result<RoutePlan, GatewayError>;
}
