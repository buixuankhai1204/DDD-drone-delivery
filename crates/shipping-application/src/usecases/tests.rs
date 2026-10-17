use std::sync::Arc;

use shipping_domain::{DomainError, DroneId, DroneProfile, ShipmentId, ShipmentStatus};
use uuid::Uuid;

use super::*;
use crate::{
    dto::{
        CancelShipmentCommand, ListShipmentsQuery, LocationInput, ParcelInput, RecipientInput,
        ReportDeliveryFailureCommand, RequestShipmentCommand, ShipmentView,
    },
    error::ApplicationError,
    testing::{FakeFleet, FakePlanner, InMemoryShipments},
};

struct Harness {
    shipments: Arc<InMemoryShipments>,
    fleet: Arc<FakeFleet>,
    request: RequestShipment,
    dispatch: DispatchShipment,
    cancel: CancelShipment,
    pickup: RecordPickup,
    deliver: RecordDelivery,
    fail: ReportDeliveryFailure,
}

fn drone(payload: u32, range: u32) -> DroneProfile {
    DroneProfile {
        id: DroneId::new(),
        max_payload_grams: payload,
        max_range_m: range,
    }
}

fn harness(drones: Vec<DroneProfile>, planner: FakePlanner) -> Harness {
    let shipments = Arc::new(InMemoryShipments::default());
    let fleet = Arc::new(FakeFleet::with_drones(drones));
    let planner = Arc::new(planner);
    Harness {
        request: RequestShipment::new(shipments.clone()),
        dispatch: DispatchShipment::new(shipments.clone(), fleet.clone(), planner),
        cancel: CancelShipment::new(shipments.clone(), fleet.clone()),
        pickup: RecordPickup::new(shipments.clone()),
        deliver: RecordDelivery::new(shipments.clone(), fleet.clone()),
        fail: ReportDeliveryFailure::new(shipments.clone(), fleet.clone()),
        shipments,
        fleet,
    }
}

fn default_harness() -> Harness {
    harness(vec![drone(3_000, 10_000)], FakePlanner::flying(3_200, 420))
}

fn command() -> RequestShipmentCommand {
    RequestShipmentCommand {
        customer_id: Uuid::now_v7(),
        recipient: RecipientInput {
            name: "Nguyen Van A".into(),
            phone: "+84901234567".into(),
        },
        parcel: ParcelInput {
            weight_grams: 1_500,
            length_mm: 200,
            width_mm: 150,
            height_mm: 100,
            description: "Phone case".into(),
            fragile: false,
        },
        pickup: LocationInput {
            latitude: 21.0285,
            longitude: 105.8542,
            address: "Hoan Kiem, Hanoi".into(),
        },
        dropoff: LocationInput {
            latitude: 21.0380,
            longitude: 105.8340,
            address: "Ba Dinh, Hanoi".into(),
        },
    }
}

async fn requested(h: &Harness) -> ShipmentView {
    h.request.execute(command()).await.unwrap()
}

#[tokio::test]
async fn request_persists_a_requested_shipment() {
    let h = default_harness();
    let view = requested(&h).await;

    assert_eq!(view.status, ShipmentStatus::Requested);
    assert!(view.tracking_number.starts_with("DRN-"));
    assert!(h.shipments.get(ShipmentId::from(view.id)).is_some());
}

#[tokio::test]
async fn request_rejects_invalid_input_without_saving() {
    let h = default_harness();
    let mut bad = command();
    bad.parcel.weight_grams = 50_000;

    let err = h.request.execute(bad).await.unwrap_err();
    assert!(matches!(
        err,
        ApplicationError::Domain(DomainError::InvalidParcelWeight { .. })
    ));
}

#[tokio::test]
async fn dispatch_reserves_a_drone_and_schedules_the_shipment() {
    let h = default_harness();
    let created = requested(&h).await;

    let view = h.dispatch.execute(created.id).await.unwrap();

    assert_eq!(view.status, ShipmentStatus::Scheduled);
    let drone_id = view.drone_id.expect("drone assigned");
    assert!(
        h.fleet
            .reserved()
            .contains_key(&DroneId::from_uuid(drone_id))
    );
    assert_eq!(view.route.unwrap().distance_m, 3_200);
    assert!(view.estimated_delivery_at.is_some());
}

#[tokio::test]
async fn dispatch_without_drones_fails_and_leaves_shipment_requested() {
    let h = harness(vec![], FakePlanner::flying(3_200, 420));
    let created = requested(&h).await;

    let err = h.dispatch.execute(created.id).await.unwrap_err();

    assert!(matches!(err, ApplicationError::NoDroneAvailable));
    let stored = h.shipments.get(ShipmentId::from(created.id)).unwrap();
    assert_eq!(stored.status(), ShipmentStatus::Requested);
}

#[tokio::test]
async fn dispatch_skips_drones_that_cannot_fly_the_route() {
    let short_range = drone(3_000, 1_000);
    let capable = drone(3_000, 10_000);
    let h = harness(vec![short_range, capable], FakePlanner::flying(3_200, 420));
    let created = requested(&h).await;

    let view = h.dispatch.execute(created.id).await.unwrap();

    assert_eq!(view.drone_id, Some(capable.id.as_uuid()));
    assert!(!h.fleet.reserved().contains_key(&short_range.id));
}

#[tokio::test]
async fn dispatch_falls_back_when_a_reservation_is_rejected() {
    let taken = drone(3_000, 10_000);
    let free = drone(3_000, 10_000);
    let h = harness(vec![taken, free], FakePlanner::flying(3_200, 420));
    h.fleet.reject_reservation_for(taken.id);
    let created = requested(&h).await;

    let view = h.dispatch.execute(created.id).await.unwrap();

    assert_eq!(view.drone_id, Some(free.id.as_uuid()));
}

#[tokio::test]
async fn dispatch_surfaces_missing_route_without_reserving_anything() {
    let h = harness(
        vec![drone(3_000, 10_000)],
        FakePlanner::no_route("restricted airspace"),
    );
    let created = requested(&h).await;

    let err = h.dispatch.execute(created.id).await.unwrap_err();

    assert!(matches!(err, ApplicationError::RouteUnavailable(_)));
    assert!(h.fleet.reserved().is_empty());
}

#[tokio::test]
async fn dispatch_releases_the_drone_when_saving_fails() {
    let h = default_harness();
    let created = requested(&h).await;
    h.shipments.fail_next_save();

    let err = h.dispatch.execute(created.id).await.unwrap_err();

    assert!(matches!(err, ApplicationError::ConcurrentModification));
    assert!(h.fleet.reserved().is_empty());
    assert_eq!(h.fleet.released().len(), 1);
}

#[tokio::test]
async fn dispatching_twice_is_an_invalid_transition() {
    let h = default_harness();
    let created = requested(&h).await;
    h.dispatch.execute(created.id).await.unwrap();

    let err = h.dispatch.execute(created.id).await.unwrap_err();

    assert!(matches!(
        err,
        ApplicationError::Domain(DomainError::InvalidStateTransition { .. })
    ));
}

#[tokio::test]
async fn dispatch_unknown_shipment_is_not_found() {
    let h = default_harness();
    let err = h.dispatch.execute(Uuid::now_v7()).await.unwrap_err();
    assert!(matches!(err, ApplicationError::ShipmentNotFound(_)));
}

#[tokio::test]
async fn full_delivery_flow_releases_the_drone_at_the_end() {
    let h = default_harness();
    let created = requested(&h).await;
    h.dispatch.execute(created.id).await.unwrap();

    h.pickup.execute(created.id).await.unwrap();
    let delivered = h.deliver.execute(created.id).await.unwrap();

    assert_eq!(delivered.status, ShipmentStatus::Delivered);
    assert_eq!(delivered.history.len(), 4);
    assert!(h.fleet.reserved().is_empty());
}

#[tokio::test]
async fn failed_delivery_releases_the_drone() {
    let h = default_harness();
    let created = requested(&h).await;
    h.dispatch.execute(created.id).await.unwrap();
    h.pickup.execute(created.id).await.unwrap();

    let failed = h
        .fail
        .execute(ReportDeliveryFailureCommand {
            shipment_id: created.id,
            reason: "Recipient unreachable".into(),
        })
        .await
        .unwrap();

    assert_eq!(failed.status, ShipmentStatus::Failed);
    assert!(h.fleet.reserved().is_empty());
}

#[tokio::test]
async fn cancelling_a_scheduled_shipment_frees_the_drone() {
    let h = default_harness();
    let created = requested(&h).await;
    h.dispatch.execute(created.id).await.unwrap();

    let cancelled = h
        .cancel
        .execute(CancelShipmentCommand {
            shipment_id: created.id,
            reason: Some("Changed my mind".into()),
        })
        .await
        .unwrap();

    assert_eq!(cancelled.status, ShipmentStatus::Cancelled);
    assert!(h.fleet.reserved().is_empty());

    // The freed drone can serve the next shipment.
    let next = requested(&h).await;
    assert_eq!(
        h.dispatch.execute(next.id).await.unwrap().status,
        ShipmentStatus::Scheduled
    );
}

#[tokio::test]
async fn cannot_cancel_once_in_transit() {
    let h = default_harness();
    let created = requested(&h).await;
    h.dispatch.execute(created.id).await.unwrap();
    h.pickup.execute(created.id).await.unwrap();

    let err = h
        .cancel
        .execute(CancelShipmentCommand {
            shipment_id: created.id,
            reason: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        ApplicationError::Domain(DomainError::InvalidStateTransition { .. })
    ));
}

#[tokio::test]
async fn queries_find_shipments_by_id_tracking_number_and_filter() {
    let h = default_harness();
    let first = requested(&h).await;
    let second = requested(&h).await;
    h.dispatch.execute(second.id).await.unwrap();

    let shipments = h.shipments.clone();
    let get = GetShipment::new(shipments.clone());
    let track = TrackShipment::new(shipments.clone());
    let list = ListShipments::new(shipments);

    assert_eq!(get.execute(first.id).await.unwrap().id, first.id);

    let tracked = track
        .execute(&first.tracking_number.to_lowercase())
        .await
        .unwrap();
    assert_eq!(tracked.status, ShipmentStatus::Requested);

    assert!(matches!(
        track.execute("nonsense").await,
        Err(ApplicationError::Domain(DomainError::InvalidTrackingNumber))
    ));
    assert!(matches!(
        track.execute("DRN-000000000000").await,
        Err(ApplicationError::ShipmentNotFound(_))
    ));

    let scheduled = list
        .execute(ListShipmentsQuery {
            status: Some(ShipmentStatus::Scheduled),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(scheduled.total, 1);
    assert_eq!(scheduled.items[0].id, second.id);

    let page = list
        .execute(ListShipmentsQuery {
            limit: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!((page.items.len(), page.total, page.limit), (1, 2, 1));
}
