//! Domain layer of the **Shipping** bounded context.
//!
//! Pure business model: no I/O, no framework types. Everything outside the
//! context (fleet, flight planning, ...) is reached through ports defined in the
//! application layer, never from here.

pub mod error;
pub mod events;
pub mod repository;
pub mod services;
pub mod shipment;
pub mod status;
pub mod value_objects;

pub use error::DomainError;
pub use events::ShipmentEvent;
pub use repository::{Page, Paged, RepositoryError, ShipmentFilter, ShipmentRepository};
pub use services::dispatch_policy::DispatchPolicy;
pub use shipment::{
    MAX_SERVICE_DISTANCE_M, MIN_SERVICE_DISTANCE_M, NewShipment, Shipment, ShipmentParts,
    TrackingEntry,
};
pub use status::ShipmentStatus;
pub use value_objects::{
    CustomerId, Dimensions, DroneAssignment, DroneId, DroneProfile, GeoPoint, Location, Parcel,
    Recipient, RoutePlan, ShipmentId, TrackingNumber,
};
