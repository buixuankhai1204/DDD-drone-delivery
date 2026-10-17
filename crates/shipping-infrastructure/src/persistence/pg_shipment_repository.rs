use async_trait::async_trait;
use chrono::{DateTime, Utc};
use shipping_domain::{
    CustomerId, Dimensions, DomainError, DroneAssignment, DroneId, GeoPoint, Location, Page, Paged,
    Parcel, Recipient, RepositoryError, RoutePlan, Shipment, ShipmentEvent, ShipmentFilter,
    ShipmentId, ShipmentParts, ShipmentRepository, TrackingEntry, TrackingNumber,
};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

const SELECT_SHIPMENTS: &str = "
    SELECT id, tracking_number, customer_id, status,
           recipient_name, recipient_phone,
           parcel_weight_grams, parcel_length_mm, parcel_width_mm, parcel_height_mm,
           parcel_description, parcel_fragile,
           pickup_latitude, pickup_longitude, pickup_address,
           dropoff_latitude, dropoff_longitude, dropoff_address,
           drone_id, assigned_at, route_distance_m, route_duration_secs, estimated_delivery_at,
           history, requested_at, updated_at, version
    FROM shipments";

const FILTER: &str = "
    WHERE ($1::uuid IS NULL OR customer_id = $1)
      AND ($2::text IS NULL OR status = $2)";

/// PostgreSQL adapter of [`ShipmentRepository`].
#[derive(Clone)]
pub struct PgShipmentRepository {
    pool: PgPool,
}

impl PgShipmentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ShipmentRepository for PgShipmentRepository {
    async fn find_by_id(&self, id: ShipmentId) -> Result<Option<Shipment>, RepositoryError> {
        let row = sqlx::query_as::<_, ShipmentRow>(&format!("{SELECT_SHIPMENTS} WHERE id = $1"))
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(RepositoryError::storage)?;
        row.map(ShipmentRow::into_shipment).transpose()
    }

    async fn find_by_tracking_number(
        &self,
        tracking_number: &TrackingNumber,
    ) -> Result<Option<Shipment>, RepositoryError> {
        let row = sqlx::query_as::<_, ShipmentRow>(&format!(
            "{SELECT_SHIPMENTS} WHERE tracking_number = $1"
        ))
        .bind(tracking_number.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(RepositoryError::storage)?;
        row.map(ShipmentRow::into_shipment).transpose()
    }

    async fn search(
        &self,
        filter: ShipmentFilter,
        page: Page,
    ) -> Result<Paged<Shipment>, RepositoryError> {
        let customer_id = filter.customer_id.map(|c| c.as_uuid());
        let status = filter.status.map(|s| s.as_str());

        let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM shipments {FILTER}"))
            .bind(customer_id)
            .bind(status)
            .fetch_one(&self.pool)
            .await
            .map_err(RepositoryError::storage)?;

        let rows = sqlx::query_as::<_, ShipmentRow>(&format!(
            "{SELECT_SHIPMENTS} {FILTER} ORDER BY requested_at DESC, id DESC LIMIT $3 OFFSET $4"
        ))
        .bind(customer_id)
        .bind(status)
        .bind(i64::from(page.limit))
        .bind(i64::from(page.offset))
        .fetch_all(&self.pool)
        .await
        .map_err(RepositoryError::storage)?;

        Ok(Paged {
            items: rows
                .into_iter()
                .map(ShipmentRow::into_shipment)
                .collect::<Result<_, _>>()?,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }

    async fn save(&self, shipment: &mut Shipment) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(RepositoryError::storage)?;

        if shipment.is_new() {
            insert_shipment(&mut tx, shipment).await?;
        } else {
            update_shipment(&mut tx, shipment).await?;
        }
        insert_outbox_events(&mut tx, shipment.pending_events()).await?;

        tx.commit().await.map_err(RepositoryError::storage)?;
        shipment.mark_persisted();
        Ok(())
    }
}

async fn insert_shipment(
    tx: &mut Transaction<'_, Postgres>,
    shipment: &Shipment,
) -> Result<(), RepositoryError> {
    let parcel = shipment.parcel();
    let dimensions = parcel.dimensions();
    let (pickup, dropoff) = (shipment.pickup(), shipment.dropoff());
    let assignment = shipment.assignment();
    let route = shipment.route();

    let result = sqlx::query(
        "INSERT INTO shipments (
            id, tracking_number, customer_id, status,
            recipient_name, recipient_phone,
            parcel_weight_grams, parcel_length_mm, parcel_width_mm, parcel_height_mm,
            parcel_description, parcel_fragile,
            pickup_latitude, pickup_longitude, pickup_address,
            dropoff_latitude, dropoff_longitude, dropoff_address,
            drone_id, assigned_at, route_distance_m, route_duration_secs, estimated_delivery_at,
            history, requested_at, updated_at, version
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
            $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27
        )",
    )
    .bind(shipment.id().as_uuid())
    .bind(shipment.tracking_number().as_str())
    .bind(shipment.customer_id().as_uuid())
    .bind(shipment.status().as_str())
    .bind(shipment.recipient().name())
    .bind(shipment.recipient().phone())
    .bind(to_db_int(parcel.weight_grams()))
    .bind(to_db_int(dimensions.length_mm()))
    .bind(to_db_int(dimensions.width_mm()))
    .bind(to_db_int(dimensions.height_mm()))
    .bind(parcel.description())
    .bind(parcel.is_fragile())
    .bind(pickup.coordinates().latitude())
    .bind(pickup.coordinates().longitude())
    .bind(pickup.address())
    .bind(dropoff.coordinates().latitude())
    .bind(dropoff.coordinates().longitude())
    .bind(dropoff.address())
    .bind(assignment.map(|a| a.drone_id.as_uuid()))
    .bind(assignment.map(|a| a.assigned_at))
    .bind(route.map(|r| to_db_int(r.distance_m())))
    .bind(route.map(|r| to_db_int(r.duration_secs())))
    .bind(shipment.estimated_delivery_at())
    .bind(Json(shipment.history()))
    .bind(shipment.requested_at())
    .bind(shipment.updated_at())
    .bind(shipment.version() + 1)
    .execute(&mut **tx)
    .await;

    match result {
        Ok(_) => Ok(()),
        // Practically only possible for a tracking-number collision.
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(RepositoryError::Conflict(shipment.id()))
        }
        Err(e) => Err(RepositoryError::storage(e)),
    }
}

/// Only the parts of the aggregate that can change after creation.
async fn update_shipment(
    tx: &mut Transaction<'_, Postgres>,
    shipment: &Shipment,
) -> Result<(), RepositoryError> {
    let assignment = shipment.assignment();
    let route = shipment.route();

    let result = sqlx::query(
        "UPDATE shipments SET
            status = $3,
            drone_id = $4, assigned_at = $5,
            route_distance_m = $6, route_duration_secs = $7, estimated_delivery_at = $8,
            history = $9, updated_at = $10,
            version = version + 1
         WHERE id = $1 AND version = $2",
    )
    .bind(shipment.id().as_uuid())
    .bind(shipment.version())
    .bind(shipment.status().as_str())
    .bind(assignment.map(|a| a.drone_id.as_uuid()))
    .bind(assignment.map(|a| a.assigned_at))
    .bind(route.map(|r| to_db_int(r.distance_m())))
    .bind(route.map(|r| to_db_int(r.duration_secs())))
    .bind(shipment.estimated_delivery_at())
    .bind(Json(shipment.history()))
    .bind(shipment.updated_at())
    .execute(&mut **tx)
    .await
    .map_err(RepositoryError::storage)?;

    if result.rows_affected() == 0 {
        return Err(RepositoryError::Conflict(shipment.id()));
    }
    Ok(())
}

async fn insert_outbox_events(
    tx: &mut Transaction<'_, Postgres>,
    events: &[ShipmentEvent],
) -> Result<(), RepositoryError> {
    for event in events {
        let payload = serde_json::to_value(event).map_err(RepositoryError::storage)?;
        sqlx::query(
            "INSERT INTO outbox_events (id, aggregate_id, event_type, payload, occurred_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(event.shipment_id().as_uuid())
        .bind(event.event_type())
        .bind(payload)
        .bind(event.occurred_at())
        .execute(&mut **tx)
        .await
        .map_err(RepositoryError::storage)?;
    }
    Ok(())
}

/// Domain values are range-checked far below `i32::MAX`; saturate defensively.
fn to_db_int(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

// ---- row mapping -----------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum RowError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("column {0} holds a negative value")]
    Negative(&'static str),
}

fn from_db_int(column: &'static str, value: i32) -> Result<u32, RowError> {
    u32::try_from(value).map_err(|_| RowError::Negative(column))
}

#[derive(sqlx::FromRow)]
struct ShipmentRow {
    id: Uuid,
    tracking_number: String,
    customer_id: Uuid,
    status: String,
    recipient_name: String,
    recipient_phone: String,
    parcel_weight_grams: i32,
    parcel_length_mm: i32,
    parcel_width_mm: i32,
    parcel_height_mm: i32,
    parcel_description: String,
    parcel_fragile: bool,
    pickup_latitude: f64,
    pickup_longitude: f64,
    pickup_address: String,
    dropoff_latitude: f64,
    dropoff_longitude: f64,
    dropoff_address: String,
    drone_id: Option<Uuid>,
    assigned_at: Option<DateTime<Utc>>,
    route_distance_m: Option<i32>,
    route_duration_secs: Option<i32>,
    estimated_delivery_at: Option<DateTime<Utc>>,
    history: Json<Vec<TrackingEntry>>,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

impl ShipmentRow {
    fn into_shipment(self) -> Result<Shipment, RepositoryError> {
        self.try_into_shipment().map_err(RepositoryError::storage)
    }

    fn try_into_shipment(self) -> Result<Shipment, RowError> {
        let dimensions = Dimensions::new(
            from_db_int("parcel_length_mm", self.parcel_length_mm)?,
            from_db_int("parcel_width_mm", self.parcel_width_mm)?,
            from_db_int("parcel_height_mm", self.parcel_height_mm)?,
        )?;
        let parcel = Parcel::new(
            from_db_int("parcel_weight_grams", self.parcel_weight_grams)?,
            dimensions,
            self.parcel_description,
            self.parcel_fragile,
        )?;

        let assignment = match (self.drone_id, self.assigned_at) {
            (Some(drone_id), Some(assigned_at)) => Some(DroneAssignment {
                drone_id: DroneId::from_uuid(drone_id),
                assigned_at,
            }),
            _ => None,
        };
        let route = match (self.route_distance_m, self.route_duration_secs) {
            (Some(distance), Some(duration)) => Some(RoutePlan::new(
                from_db_int("route_distance_m", distance)?,
                from_db_int("route_duration_secs", duration)?,
            )?),
            _ => None,
        };

        Ok(Shipment::restore(ShipmentParts {
            id: ShipmentId::from_uuid(self.id),
            tracking_number: TrackingNumber::parse(&self.tracking_number)?,
            customer_id: CustomerId::from_uuid(self.customer_id),
            recipient: Recipient::new(self.recipient_name, self.recipient_phone)?,
            parcel,
            pickup: Location::new(
                GeoPoint::new(self.pickup_latitude, self.pickup_longitude)?,
                self.pickup_address,
            )?,
            dropoff: Location::new(
                GeoPoint::new(self.dropoff_latitude, self.dropoff_longitude)?,
                self.dropoff_address,
            )?,
            status: self.status.parse()?,
            assignment,
            route,
            estimated_delivery_at: self.estimated_delivery_at,
            history: self.history.0,
            requested_at: self.requested_at,
            updated_at: self.updated_at,
            version: self.version,
        }))
    }
}
