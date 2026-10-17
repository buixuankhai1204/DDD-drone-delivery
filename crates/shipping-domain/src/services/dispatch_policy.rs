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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::{Dimensions, DroneId};

    fn parcel(weight: u32) -> Parcel {
        Parcel::new(
            weight,
            Dimensions::new(100, 100, 100).unwrap(),
            "box",
            false,
        )
        .unwrap()
    }

    fn drone(payload: u32, range: u32) -> DroneProfile {
        DroneProfile {
            id: DroneId::new(),
            max_payload_grams: payload,
            max_range_m: range,
        }
    }

    #[test]
    fn accepts_suitable_drone() {
        let route = RoutePlan::new(4_000, 600).unwrap();
        assert!(DispatchPolicy::verify(&parcel(2_000), &route, &drone(3_000, 10_000)).is_ok());
    }

    #[test]
    fn rejects_overweight_parcel() {
        let route = RoutePlan::new(4_000, 600).unwrap();
        assert!(matches!(
            DispatchPolicy::verify(&parcel(4_000), &route, &drone(3_000, 10_000)),
            Err(DomainError::DroneCannotCarryParcel { .. })
        ));
    }

    #[test]
    fn rejects_route_beyond_range() {
        let route = RoutePlan::new(12_000, 900).unwrap();
        assert!(matches!(
            DispatchPolicy::verify(&parcel(1_000), &route, &drone(3_000, 10_000)),
            Err(DomainError::RouteExceedsDroneRange { .. })
        ));
    }
}
