//! Use cases that move a shipment through the rest of its lifecycle.

use std::sync::Arc;

use chrono::Utc;
use shipping_domain::ShipmentRepository;
use uuid::Uuid;

use super::{load_shipment, release_drone_quietly};
use crate::{
    dto::{CancelShipmentCommand, ReportDeliveryFailureCommand, ShipmentView},
    error::ApplicationError,
    ports::FleetGateway,
};

/// Customer or operator cancels a shipment that has not taken off yet.
pub struct CancelShipment {
    shipments: Arc<dyn ShipmentRepository>,
    fleet: Arc<dyn FleetGateway>,
}

impl CancelShipment {
    pub fn new(shipments: Arc<dyn ShipmentRepository>, fleet: Arc<dyn FleetGateway>) -> Self {
        Self { shipments, fleet }
    }

    #[tracing::instrument(skip_all, fields(shipment_id = %cmd.shipment_id))]
    pub async fn execute(
        &self,
        cmd: CancelShipmentCommand,
    ) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = load_shipment(&*self.shipments, cmd.shipment_id).await?;
        shipment.cancel(cmd.reason.as_deref(), Utc::now())?;
        self.shipments.save(&mut shipment).await?;

        release_drone_quietly(&*self.fleet, &shipment).await;
        Ok(ShipmentView::from(&shipment))
    }
}

/// The drone reports it collected the parcel.
pub struct RecordPickup {
    shipments: Arc<dyn ShipmentRepository>,
}

impl RecordPickup {
    pub fn new(shipments: Arc<dyn ShipmentRepository>) -> Self {
        Self { shipments }
    }

    #[tracing::instrument(skip(self))]
    pub async fn execute(&self, shipment_id: Uuid) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = load_shipment(&*self.shipments, shipment_id).await?;
        shipment.record_pickup(Utc::now())?;
        self.shipments.save(&mut shipment).await?;
        Ok(ShipmentView::from(&shipment))
    }
}

/// The drone reports the parcel arrived at the dropoff point.
pub struct RecordDelivery {
    shipments: Arc<dyn ShipmentRepository>,
    fleet: Arc<dyn FleetGateway>,
}

impl RecordDelivery {
    pub fn new(shipments: Arc<dyn ShipmentRepository>, fleet: Arc<dyn FleetGateway>) -> Self {
        Self { shipments, fleet }
    }

    #[tracing::instrument(skip(self))]
    pub async fn execute(&self, shipment_id: Uuid) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = load_shipment(&*self.shipments, shipment_id).await?;
        shipment.record_delivery(Utc::now())?;
        self.shipments.save(&mut shipment).await?;

        release_drone_quietly(&*self.fleet, &shipment).await;
        Ok(ShipmentView::from(&shipment))
    }
}

/// The drone reports it could not complete the delivery.
pub struct ReportDeliveryFailure {
    shipments: Arc<dyn ShipmentRepository>,
    fleet: Arc<dyn FleetGateway>,
}

impl ReportDeliveryFailure {
    pub fn new(shipments: Arc<dyn ShipmentRepository>, fleet: Arc<dyn FleetGateway>) -> Self {
        Self { shipments, fleet }
    }

    #[tracing::instrument(skip_all, fields(shipment_id = %cmd.shipment_id))]
    pub async fn execute(
        &self,
        cmd: ReportDeliveryFailureCommand,
    ) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = load_shipment(&*self.shipments, cmd.shipment_id).await?;
        shipment.record_delivery_failure(&cmd.reason, Utc::now())?;
        self.shipments.save(&mut shipment).await?;

        release_drone_quietly(&*self.fleet, &shipment).await;
        Ok(ShipmentView::from(&shipment))
    }
}
