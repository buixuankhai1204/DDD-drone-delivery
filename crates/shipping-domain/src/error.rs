use crate::status::ShipmentStatus;

/// Violations of business invariants raised by the domain model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("latitude must be within [-90, 90] and longitude within [-180, 180]")]
    InvalidCoordinates,

    #[error("{0} must not be blank")]
    BlankField(&'static str),

    #[error("{field} must not be longer than {max} characters")]
    FieldTooLong { field: &'static str, max: usize },

    #[error("phone number must be in international format, e.g. +84901234567")]
    InvalidPhoneNumber,

    #[error("parcel weight must be between 1 and {max_grams} grams")]
    InvalidParcelWeight { max_grams: u32 },

    #[error("each parcel dimension must be between 1 and {max_mm} mm")]
    InvalidParcelDimensions { max_mm: u32 },

    #[error("invalid tracking number")]
    InvalidTrackingNumber,

    #[error("route distance and duration must be greater than zero")]
    InvalidRoutePlan,

    #[error("pickup and dropoff are only {distance_m} m apart (minimum {min_m} m)")]
    LocationsTooClose { distance_m: u32, min_m: u32 },

    #[error("distance of {distance_m} m exceeds the maximum service distance of {max_m} m")]
    OutsideServiceArea { distance_m: u32, max_m: u32 },

    #[error("cannot {action} a shipment that is {status}")]
    InvalidStateTransition {
        action: &'static str,
        status: ShipmentStatus,
    },

    #[error("drone payload limit is {max_payload_grams} g but the parcel weighs {weight_grams} g")]
    DroneCannotCarryParcel {
        weight_grams: u32,
        max_payload_grams: u32,
    },

    #[error("route of {distance_m} m exceeds the drone range of {max_range_m} m")]
    RouteExceedsDroneRange { distance_m: u32, max_range_m: u32 },

    #[error("shipment has no drone assigned")]
    NoDroneAssigned,

    #[error("unknown shipment status '{0}'")]
    UnknownStatus(String),
}
