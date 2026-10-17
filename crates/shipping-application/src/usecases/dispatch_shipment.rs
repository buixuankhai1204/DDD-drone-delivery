use std::sync::Arc;

use chrono::Utc;
use shipping_domain::{DroneId, Shipment, ShipmentId, ShipmentRepository};
use uuid::Uuid;

use super::load_shipment;
use crate::{
    dto::ShipmentView,
    error::ApplicationError,
    ports::{DroneRequirements, FleetGateway, GatewayError, RoutePlanner},
};

/// How many drones we are willing to try for one shipment.
const MAX_CANDIDATES: usize = 3;

/// Finds a drone for a requested shipment, plans the flight and reserves the
/// drone.
///
/// Talks to two other bounded contexts, so the steps are ordered so that
/// nothing needs undoing until the drone is reserved; the only compensation
/// is releasing that reservation when our own save fails.
pub struct DispatchShipment {
    shipments: Arc<dyn ShipmentRepository>,
    fleet: Arc<dyn FleetGateway>,
    routes: Arc<dyn RoutePlanner>,
}

impl DispatchShipment {
    pub fn new(
        shipments: Arc<dyn ShipmentRepository>,
        fleet: Arc<dyn FleetGateway>,
        routes: Arc<dyn RoutePlanner>,
    ) -> Self {
        Self {
            shipments,
            fleet,
            routes,
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn execute(&self, shipment_id: Uuid) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = load_shipment(&*self.shipments, shipment_id).await?;
        shipment.ensure_awaiting_dispatch()?;

        let requirements = DroneRequirements {
            payload_grams: shipment.parcel().weight_grams(),
            pickup: *shipment.pickup().coordinates(),
        };
        let candidates = self
            .fleet
            .find_available_drones(&requirements, MAX_CANDIDATES)
            .await?;

        for drone in candidates {
            let route = match self
                .routes
                .plan_route(
                    drone.id,
                    shipment.pickup().coordinates(),
                    shipment.dropoff().coordinates(),
                )
                .await
            {
                Ok(route) => route,
                // No flyable route; another drone will not change that.
                Err(GatewayError::Rejected { reason, .. }) => {
                    return Err(ApplicationError::RouteUnavailable(reason));
                }
                Err(error) => return Err(error.into()),
            };

            if let Err(reason) = shipment.ensure_can_schedule(&drone, &route) {
                tracing::debug!(drone_id = %drone.id, %reason, "drone not suitable, trying next");
                continue;
            }

            match self.fleet.reserve_drone(drone.id, shipment.id()).await {
                Ok(()) => {}
                Err(GatewayError::Rejected { reason, .. }) => {
                    tracing::debug!(drone_id = %drone.id, %reason, "drone taken, trying next");
                    continue;
                }
                Err(error) => return Err(error.into()),
            }

            shipment.schedule(&drone, route, Utc::now())?;
            if let Err(error) = self.shipments.save(&mut shipment).await {
                self.undo_reservation(drone.id, shipment.id()).await;
                return Err(error.into());
            }

            tracing::info!(drone_id = %drone.id, "shipment scheduled");
            return Ok(ShipmentView::from(&shipment));
        }

        Err(ApplicationError::NoDroneAvailable)
    }

    /// Releases a reservation whose shipment could not be saved.
    ///
    /// A concurrent dispatch may have won with the *same* drone (reserving is
    /// idempotent per shipment), in which case the reservation is theirs now
    /// and must stay.
    async fn undo_reservation(&self, drone_id: DroneId, shipment_id: ShipmentId) {
        let current: Option<Shipment> = match self.shipments.find_by_id(shipment_id).await {
            Ok(shipment) => shipment,
            Err(error) => {
                tracing::warn!(%drone_id, %shipment_id, %error,
                    "cannot tell who owns the reservation, leaving it for Fleet to expire");
                return;
            }
        };
        if current.and_then(|s| s.assigned_drone()) == Some(drone_id) {
            return;
        }
        if let Err(error) = self.fleet.release_drone(drone_id, shipment_id).await {
            tracing::warn!(%drone_id, %shipment_id, %error, "failed to release reservation");
        }
    }
}
