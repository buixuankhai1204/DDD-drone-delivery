//! Data contracts of the use cases: commands/queries going in, views coming
//! out. Plain data, no domain types, so adapters stay decoupled from the model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shipping_domain::{
    Dimensions, DomainError, GeoPoint, Location, Parcel, Recipient, Shipment, ShipmentStatus,
    TrackingEntry,
};
use uuid::Uuid;

// ---- commands ------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RequestShipmentCommand {
    pub customer_id: Uuid,
    pub recipient: RecipientInput,
    pub parcel: ParcelInput,
    pub pickup: LocationInput,
    pub dropoff: LocationInput,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipientInput {
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParcelInput {
    pub weight_grams: u32,
    pub length_mm: u32,
    pub width_mm: u32,
    pub height_mm: u32,
    pub description: String,
    #[serde(default)]
    pub fragile: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocationInput {
    pub latitude: f64,
    pub longitude: f64,
    pub address: String,
}

impl RecipientInput {
    pub(crate) fn into_domain(self) -> Result<Recipient, DomainError> {
        Recipient::new(self.name, self.phone)
    }
}

impl ParcelInput {
    pub(crate) fn into_domain(self) -> Result<Parcel, DomainError> {
        let dimensions = Dimensions::new(self.length_mm, self.width_mm, self.height_mm)?;
        Parcel::new(
            self.weight_grams,
            dimensions,
            self.description,
            self.fragile,
        )
    }
}

impl LocationInput {
    pub(crate) fn into_domain(self) -> Result<Location, DomainError> {
        Location::new(GeoPoint::new(self.latitude, self.longitude)?, self.address)
    }
}

#[derive(Debug, Clone)]
pub struct CancelShipmentCommand {
    pub shipment_id: Uuid,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReportDeliveryFailureCommand {
    pub shipment_id: Uuid,
    pub reason: String,
}

// ---- queries -------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ListShipmentsQuery {
    pub customer_id: Option<Uuid>,
    pub status: Option<ShipmentStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ---- views ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ShipmentView {
    pub id: Uuid,
    pub tracking_number: String,
    pub customer_id: Uuid,
    pub status: ShipmentStatus,
    pub recipient: RecipientView,
    pub parcel: ParcelView,
    pub pickup: LocationView,
    pub dropoff: LocationView,
    pub drone_id: Option<Uuid>,
    pub route: Option<RouteView>,
    pub estimated_delivery_at: Option<DateTime<Utc>>,
    pub history: Vec<TrackingEntryView>,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipientView {
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParcelView {
    pub weight_grams: u32,
    pub length_mm: u32,
    pub width_mm: u32,
    pub height_mm: u32,
    pub description: String,
    pub fragile: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocationView {
    pub latitude: f64,
    pub longitude: f64,
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteView {
    pub distance_m: u32,
    pub duration_secs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackingEntryView {
    pub status: ShipmentStatus,
    pub occurred_at: DateTime<Utc>,
    pub note: Option<String>,
}

/// What anybody holding a tracking number may see: no personal data.
#[derive(Debug, Clone, Serialize)]
pub struct TrackingView {
    pub tracking_number: String,
    pub status: ShipmentStatus,
    pub estimated_delivery_at: Option<DateTime<Utc>>,
    pub history: Vec<TrackingEntryView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PagedView<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

impl From<&TrackingEntry> for TrackingEntryView {
    fn from(entry: &TrackingEntry) -> Self {
        Self {
            status: entry.status,
            occurred_at: entry.occurred_at,
            note: entry.note.clone(),
        }
    }
}

fn location_view(location: &Location) -> LocationView {
    LocationView {
        latitude: location.coordinates().latitude(),
        longitude: location.coordinates().longitude(),
        address: location.address().to_owned(),
    }
}

impl From<&Shipment> for ShipmentView {
    fn from(shipment: &Shipment) -> Self {
        let parcel = shipment.parcel();
        let dimensions = parcel.dimensions();

        Self {
            id: shipment.id().as_uuid(),
            tracking_number: shipment.tracking_number().to_string(),
            customer_id: shipment.customer_id().as_uuid(),
            status: shipment.status(),
            recipient: RecipientView {
                name: shipment.recipient().name().to_owned(),
                phone: shipment.recipient().phone().to_owned(),
            },
            parcel: ParcelView {
                weight_grams: parcel.weight_grams(),
                length_mm: dimensions.length_mm(),
                width_mm: dimensions.width_mm(),
                height_mm: dimensions.height_mm(),
                description: parcel.description().to_owned(),
                fragile: parcel.is_fragile(),
            },
            pickup: location_view(shipment.pickup()),
            dropoff: location_view(shipment.dropoff()),
            drone_id: shipment.assigned_drone().map(|d| d.as_uuid()),
            route: shipment.route().map(|r| RouteView {
                distance_m: r.distance_m(),
                duration_secs: r.duration_secs(),
            }),
            estimated_delivery_at: shipment.estimated_delivery_at(),
            history: shipment.history().iter().map(Into::into).collect(),
            requested_at: shipment.requested_at(),
            updated_at: shipment.updated_at(),
            version: shipment.version(),
        }
    }
}

impl From<&Shipment> for TrackingView {
    fn from(shipment: &Shipment) -> Self {
        Self {
            tracking_number: shipment.tracking_number().to_string(),
            status: shipment.status(),
            estimated_delivery_at: shipment.estimated_delivery_at(),
            history: shipment.history().iter().map(Into::into).collect(),
        }
    }
}
