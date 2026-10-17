use std::sync::Arc;

use shipping_application::{
    ports::{FleetGateway, RoutePlanner},
    usecases::{
        CancelShipment, DispatchShipment, GetShipment, ListShipments, RecordDelivery, RecordPickup,
        ReportDeliveryFailure, RequestShipment, TrackShipment,
    },
};
use shipping_domain::ShipmentRepository;
use shipping_infrastructure::persistence::PgShipmentRepository;
use sqlx::PgPool;

/// Everything the handlers need. Cheap to clone (all `Arc`s).
#[derive(Clone)]
pub struct AppState {
    pub request_shipment: Arc<RequestShipment>,
    pub dispatch_shipment: Arc<DispatchShipment>,
    pub cancel_shipment: Arc<CancelShipment>,
    pub record_pickup: Arc<RecordPickup>,
    pub record_delivery: Arc<RecordDelivery>,
    pub report_delivery_failure: Arc<ReportDeliveryFailure>,
    pub get_shipment: Arc<GetShipment>,
    pub track_shipment: Arc<TrackShipment>,
    pub list_shipments: Arc<ListShipments>,
    pub db: PgPool,
}

impl AppState {
    /// Composition root: wires use cases to the PostgreSQL repository and the
    /// given gateways to the other bounded contexts.
    pub fn new(db: PgPool, fleet: Arc<dyn FleetGateway>, routes: Arc<dyn RoutePlanner>) -> Self {
        let shipments: Arc<dyn ShipmentRepository> =
            Arc::new(PgShipmentRepository::new(db.clone()));

        Self {
            request_shipment: Arc::new(RequestShipment::new(shipments.clone())),
            dispatch_shipment: Arc::new(DispatchShipment::new(
                shipments.clone(),
                fleet.clone(),
                routes,
            )),
            cancel_shipment: Arc::new(CancelShipment::new(shipments.clone(), fleet.clone())),
            record_pickup: Arc::new(RecordPickup::new(shipments.clone())),
            record_delivery: Arc::new(RecordDelivery::new(shipments.clone(), fleet.clone())),
            report_delivery_failure: Arc::new(ReportDeliveryFailure::new(shipments.clone(), fleet)),
            get_shipment: Arc::new(GetShipment::new(shipments.clone())),
            track_shipment: Arc::new(TrackShipment::new(shipments.clone())),
            list_shipments: Arc::new(ListShipments::new(shipments)),
            db,
        }
    }
}
