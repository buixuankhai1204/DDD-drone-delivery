//! Repository and outbox tests against a real PostgreSQL.
//!
//! They need a database and are therefore ignored by default:
//!
//! ```text
//! docker compose up -d db
//! DATABASE_URL=postgres://shipping:shipping@localhost:5432/shipping \
//!     cargo test -p shipping-infrastructure -- --include-ignored
//! ```
//!
//! `#[sqlx::test]` creates an isolated database per test and applies the
//! migrations to it.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use shipping_application::ports::{EventPublisher, OutboxMessage, PublishError};
use shipping_domain::{
    CustomerId, Dimensions, DroneId, DroneProfile, GeoPoint, Location, NewShipment, Page, Parcel,
    Recipient, RepositoryError, RoutePlan, Shipment, ShipmentFilter, ShipmentRepository,
    ShipmentStatus,
};
use shipping_infrastructure::{messaging::OutboxRelay, persistence::PgShipmentRepository};
use sqlx::PgPool;

fn shipment_for(customer_id: CustomerId) -> Shipment {
    let location =
        |lat, lon, addr: &str| Location::new(GeoPoint::new(lat, lon).unwrap(), addr).unwrap();
    Shipment::request(
        NewShipment {
            customer_id,
            recipient: Recipient::new("Nguyen Van A", "+84901234567").unwrap(),
            parcel: Parcel::new(
                1_500,
                Dimensions::new(200, 150, 100).unwrap(),
                "Phone case",
                true,
            )
            .unwrap(),
            pickup: location(21.0285, 105.8542, "Hoan Kiem, Hanoi"),
            dropoff: location(21.0380, 105.8340, "Ba Dinh, Hanoi"),
        },
        Utc::now(),
    )
    .unwrap()
}

fn drone() -> DroneProfile {
    DroneProfile {
        id: DroneId::new(),
        max_payload_grams: 3_000,
        max_range_m: 10_000,
    }
}

async fn outbox_count(pool: &PgPool, only_unpublished: bool) -> i64 {
    let sql = if only_unpublished {
        "SELECT COUNT(*) FROM outbox_events WHERE published_at IS NULL"
    } else {
        "SELECT COUNT(*) FROM outbox_events"
    };
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[derive(Default)]
struct RecordingPublisher {
    published: Mutex<Vec<OutboxMessage>>,
    failing: bool,
}

#[async_trait]
impl EventPublisher for RecordingPublisher {
    async fn publish(&self, message: &OutboxMessage) -> Result<(), PublishError> {
        if self.failing {
            return Err(PublishError("broker down".into()));
        }
        self.published.lock().unwrap().push(message.clone());
        Ok(())
    }
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn saves_and_reloads_a_new_shipment(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    let mut shipment = shipment_for(CustomerId::new());

    repo.save(&mut shipment).await.unwrap();
    assert_eq!(shipment.version(), 1);
    assert!(shipment.pending_events().is_empty());

    let by_id = repo.find_by_id(shipment.id()).await.unwrap().unwrap();
    assert_eq!(by_id.status(), ShipmentStatus::Requested);
    assert_eq!(by_id.tracking_number(), shipment.tracking_number());
    assert_eq!(by_id.parcel(), shipment.parcel());
    assert_eq!(by_id.pickup(), shipment.pickup());
    assert_eq!(by_id.dropoff(), shipment.dropoff());
    assert_eq!(by_id.recipient(), shipment.recipient());
    assert_eq!(by_id.history(), shipment.history());
    assert_eq!(by_id.version(), 1);

    let by_tracking = repo
        .find_by_tracking_number(shipment.tracking_number())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_tracking.id(), shipment.id());

    assert!(repo.find_by_id(Default::default()).await.unwrap().is_none());
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn persists_every_lifecycle_step(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    let mut shipment = shipment_for(CustomerId::new());
    repo.save(&mut shipment).await.unwrap();

    let drone = drone();
    shipment
        .schedule(&drone, RoutePlan::new(3_200, 420).unwrap(), Utc::now())
        .unwrap();
    repo.save(&mut shipment).await.unwrap();

    let scheduled = repo.find_by_id(shipment.id()).await.unwrap().unwrap();
    assert_eq!(scheduled.status(), ShipmentStatus::Scheduled);
    assert_eq!(scheduled.assigned_drone(), Some(drone.id));
    assert_eq!(scheduled.route().unwrap().distance_m(), 3_200);
    assert!(scheduled.estimated_delivery_at().is_some());
    assert_eq!(scheduled.version(), 2);

    let mut reloaded = scheduled;
    reloaded.record_pickup(Utc::now()).unwrap();
    reloaded.record_delivery(Utc::now()).unwrap();
    repo.save(&mut reloaded).await.unwrap();

    let delivered = repo.find_by_id(shipment.id()).await.unwrap().unwrap();
    assert_eq!(delivered.status(), ShipmentStatus::Delivered);
    assert_eq!(delivered.history().len(), 4);
    assert_eq!(delivered.version(), 3);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn concurrent_modification_is_detected(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    let mut shipment = shipment_for(CustomerId::new());
    repo.save(&mut shipment).await.unwrap();

    let mut first = repo.find_by_id(shipment.id()).await.unwrap().unwrap();
    let mut second = repo.find_by_id(shipment.id()).await.unwrap().unwrap();

    first.cancel(Some("first wins"), Utc::now()).unwrap();
    repo.save(&mut first).await.unwrap();

    second
        .schedule(&drone(), RoutePlan::new(1_000, 100).unwrap(), Utc::now())
        .unwrap();
    let err = repo.save(&mut second).await.unwrap_err();
    assert!(matches!(err, RepositoryError::Conflict(id) if id == shipment.id()));

    // The failed save left the aggregate untouched and wrote no events.
    assert_eq!(second.version(), 1);
    assert_eq!(second.pending_events().len(), 1);
    assert_eq!(outbox_count(&pool, false).await, 2); // requested + cancelled

    let stored = repo.find_by_id(shipment.id()).await.unwrap().unwrap();
    assert_eq!(stored.status(), ShipmentStatus::Cancelled);
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn searches_by_customer_and_status_with_paging(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    let alice = CustomerId::new();
    let bob = CustomerId::new();

    for customer in [alice, alice, alice, bob] {
        repo.save(&mut shipment_for(customer)).await.unwrap();
    }
    let mut cancelled = shipment_for(alice);
    cancelled.cancel(None, Utc::now()).unwrap();
    repo.save(&mut cancelled).await.unwrap();

    let all = repo
        .search(
            ShipmentFilter::default(),
            Page {
                limit: 100,
                offset: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!((all.items.len(), all.total), (5, 5));

    let alices = repo
        .search(
            ShipmentFilter {
                customer_id: Some(alice),
                ..Default::default()
            },
            Page {
                limit: 2,
                offset: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!((alices.items.len(), alices.total), (2, 4));
    assert!(alices.items.iter().all(|s| s.customer_id() == alice));

    let cancelled_only = repo
        .search(
            ShipmentFilter {
                status: Some(ShipmentStatus::Cancelled),
                ..Default::default()
            },
            Page {
                limit: 10,
                offset: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(cancelled_only.total, 1);

    // Newest first.
    let newest_first = repo
        .search(
            ShipmentFilter::default(),
            Page {
                limit: 5,
                offset: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(newest_first.items[0].id(), cancelled.id());
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn events_are_stored_with_the_aggregate_and_relayed_in_order(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    let mut shipment = shipment_for(CustomerId::new());
    repo.save(&mut shipment).await.unwrap();
    shipment
        .schedule(&drone(), RoutePlan::new(3_200, 420).unwrap(), Utc::now())
        .unwrap();
    repo.save(&mut shipment).await.unwrap();

    assert_eq!(outbox_count(&pool, true).await, 2);

    let publisher = Arc::new(RecordingPublisher::default());
    let relay = OutboxRelay::new(pool.clone(), publisher.clone());

    assert_eq!(relay.relay_batch().await.unwrap(), 2);
    assert_eq!(relay.relay_batch().await.unwrap(), 0);
    assert_eq!(outbox_count(&pool, true).await, 0);

    let published = publisher.published.lock().unwrap();
    let types: Vec<_> = published.iter().map(|m| m.event_type.as_str()).collect();
    assert_eq!(types, ["shipment.requested", "shipment.scheduled"]);
    assert_eq!(published[0].aggregate_id, shipment.id().as_uuid());
    assert_eq!(published[0].payload["type"], "requested");
    assert_eq!(
        published[0].payload["tracking_number"],
        shipment.tracking_number().as_str()
    );
}

#[sqlx::test(migrator = "shipping_infrastructure::database::MIGRATOR")]
#[ignore = "requires DATABASE_URL"]
async fn failed_publishing_keeps_events_for_the_next_attempt(pool: PgPool) {
    let repo = PgShipmentRepository::new(pool.clone());
    repo.save(&mut shipment_for(CustomerId::new()))
        .await
        .unwrap();

    let failing = OutboxRelay::new(
        pool.clone(),
        Arc::new(RecordingPublisher {
            failing: true,
            ..Default::default()
        }),
    );
    assert_eq!(failing.relay_batch().await.unwrap(), 0);
    assert_eq!(outbox_count(&pool, true).await, 1);

    let healthy = OutboxRelay::new(pool.clone(), Arc::new(RecordingPublisher::default()));
    assert_eq!(healthy.relay_batch().await.unwrap(), 1);
    assert_eq!(outbox_count(&pool, true).await, 0);
}
