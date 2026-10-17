use std::sync::Arc;

use shipping_domain::{CustomerId, Page, ShipmentFilter, ShipmentRepository, TrackingNumber};
use uuid::Uuid;

use super::load_shipment;
use crate::{
    dto::{ListShipmentsQuery, PagedView, ShipmentView, TrackingView},
    error::ApplicationError,
};

const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;

pub struct GetShipment {
    shipments: Arc<dyn ShipmentRepository>,
}

impl GetShipment {
    pub fn new(shipments: Arc<dyn ShipmentRepository>) -> Self {
        Self { shipments }
    }

    pub async fn execute(&self, shipment_id: Uuid) -> Result<ShipmentView, ApplicationError> {
        let shipment = load_shipment(&*self.shipments, shipment_id).await?;
        Ok(ShipmentView::from(&shipment))
    }
}

/// Public tracking by tracking number.
pub struct TrackShipment {
    shipments: Arc<dyn ShipmentRepository>,
}

impl TrackShipment {
    pub fn new(shipments: Arc<dyn ShipmentRepository>) -> Self {
        Self { shipments }
    }

    pub async fn execute(&self, tracking_number: &str) -> Result<TrackingView, ApplicationError> {
        let parsed = TrackingNumber::parse(tracking_number)?;
        let shipment = self
            .shipments
            .find_by_tracking_number(&parsed)
            .await?
            .ok_or_else(|| ApplicationError::ShipmentNotFound(parsed.to_string()))?;
        Ok(TrackingView::from(&shipment))
    }
}

pub struct ListShipments {
    shipments: Arc<dyn ShipmentRepository>,
}

impl ListShipments {
    pub fn new(shipments: Arc<dyn ShipmentRepository>) -> Self {
        Self { shipments }
    }

    pub async fn execute(
        &self,
        query: ListShipmentsQuery,
    ) -> Result<PagedView<ShipmentView>, ApplicationError> {
        let page = Page {
            limit: query
                .limit
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .clamp(1, MAX_PAGE_SIZE),
            offset: query.offset.unwrap_or(0),
        };
        let filter = ShipmentFilter {
            customer_id: query.customer_id.map(CustomerId::from),
            status: query.status,
        };

        let found = self.shipments.search(filter, page).await?;
        Ok(PagedView {
            items: found.items.iter().map(ShipmentView::from).collect(),
            total: found.total,
            limit: page.limit,
            offset: page.offset,
        })
    }
}
