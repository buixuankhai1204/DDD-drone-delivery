use std::sync::Arc;

use chrono::Utc;
use shipping_domain::{CustomerId, NewShipment, Shipment, ShipmentRepository};

use crate::{
    dto::{RequestShipmentCommand, ShipmentView},
    error::ApplicationError,
};

/// A customer asks for a parcel to be delivered by drone.
pub struct RequestShipment {
    shipments: Arc<dyn ShipmentRepository>,
}

impl RequestShipment {
    pub fn new(shipments: Arc<dyn ShipmentRepository>) -> Self {
        Self { shipments }
    }

    #[tracing::instrument(skip_all, fields(customer_id = %cmd.customer_id))]
    pub async fn execute(
        &self,
        cmd: RequestShipmentCommand,
    ) -> Result<ShipmentView, ApplicationError> {
        let mut shipment = Shipment::request(
            NewShipment {
                customer_id: CustomerId::from(cmd.customer_id),
                recipient: cmd.recipient.into_domain()?,
                parcel: cmd.parcel.into_domain()?,
                pickup: cmd.pickup.into_domain()?,
                dropoff: cmd.dropoff.into_domain()?,
            },
            Utc::now(),
        )?;

        self.shipments.save(&mut shipment).await?;

        tracing::info!(
            shipment_id = %shipment.id(),
            tracking_number = %shipment.tracking_number(),
            "shipment requested"
        );
        Ok(ShipmentView::from(&shipment))
    }
}
