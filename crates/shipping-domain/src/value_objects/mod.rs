mod drone;
mod geo;
mod ids;
mod parcel;
mod recipient;
mod route;
mod tracking_number;

pub use drone::{DroneAssignment, DroneProfile};
pub use geo::{GeoPoint, Location};
pub use ids::{CustomerId, DroneId, ShipmentId};
pub use parcel::{Dimensions, MAX_PARCEL_DIMENSION_MM, MAX_PARCEL_WEIGHT_GRAMS, Parcel};
pub use recipient::Recipient;
pub use route::RoutePlan;
pub use tracking_number::TrackingNumber;

use crate::error::DomainError;

/// Trims `value` and rejects it when empty or longer than `max` characters.
pub(crate) fn non_blank(
    field: &'static str,
    value: impl AsRef<str>,
    max: usize,
) -> Result<String, DomainError> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        return Err(DomainError::BlankField(field));
    }
    if value.chars().count() > max {
        return Err(DomainError::FieldTooLong { field, max });
    }
    Ok(value.to_owned())
}
