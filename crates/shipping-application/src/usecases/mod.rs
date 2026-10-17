mod dispatch_shipment;
mod lifecycle;
mod queries;
mod request_shipment;
#[cfg(test)]
mod tests;

pub use dispatch_shipment::DispatchShipment;
pub use lifecycle::{CancelShipment, RecordDelivery, RecordPickup, ReportDeliveryFailure};
pub use queries::{GetShipment, ListShipments, TrackShipment};
pub use request_shipment::RequestShipment;

use shipping_domain::{Shipment, ShipmentId, ShipmentRepository};
use uuid::Uuid;

use crate::{error::ApplicationError, ports::FleetGateway};

async fn load_shipment(
    shipments: &dyn ShipmentRepository,
    id: Uuid,
) -> Result<Shipment, ApplicationError> {
    shipments
        .find_by_id(ShipmentId::from(id))
        .await?
        .ok_or_else(|| ApplicationError::ShipmentNotFound(id.to_string()))
}

/// Tells Fleet the shipment no longer needs its drone.
///
/// Best effort: the shipment state is already saved and `shipment.cancelled`
/// / `.delivered` / `.delivery_failed` events let Fleet release on its own,
/// so a Fleet outage must not fail the user's request.
async fn release_drone_quietly(fleet: &dyn FleetGateway, shipment: &Shipment) {
    let Some(drone_id) = shipment.assigned_drone() else {
        return;
    };
    if let Err(error) = fleet.release_drone(drone_id, shipment.id()).await {
        tracing::warn!(
            %drone_id,
            shipment_id = %shipment.id(),
            %error,
            "could not release drone, relying on event-driven release"
        );
    }
}
