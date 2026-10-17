use crate::{
    error::DomainError,
    value_objects::{DroneProfile, Parcel, RoutePlan},
};

/// Domain service: decides whether a drone is suitable for a parcel on a
/// planned route. It spans several concepts (parcel, route, drone), so it
/// belongs to neither of them.
pub struct DispatchPolicy;

impl DispatchPolicy {
    pub fn verify(
        parcel: &Parcel,
        route: &RoutePlan,
        drone: &DroneProfile,
    ) -> Result<(), DomainError> {
        if parcel.weight_grams() > drone.max_payload_grams {
            return Err(DomainError::DroneCannotCarryParcel {
                weight_grams: parcel.weight_grams(),
                max_payload_grams: drone.max_payload_grams,
            });
        }
        if route.distance_m() > drone.max_range_m {
            return Err(DomainError::RouteExceedsDroneRange {
                distance_m: route.distance_m(),
                max_range_m: drone.max_range_m,
            });
        }
        Ok(())
    }
}
