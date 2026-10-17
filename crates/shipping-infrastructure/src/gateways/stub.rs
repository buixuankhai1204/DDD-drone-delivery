//! In-process stand-ins for the Fleet and Flight Planning contexts, so the
//! Shipping service can run on its own. Not meant for production.

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use shipping_application::ports::{DroneRequirements, FleetGateway, GatewayError, RoutePlanner};
use shipping_domain::{DroneId, DroneProfile, GeoPoint, RoutePlan, ShipmentId};
use uuid::Uuid;

const FLEET: &str = "fleet (stub)";
const PLANNER: &str = "flight-planning (stub)";

/// Three fixed drones; reservations are kept in memory.
pub struct StubFleetGateway {
    drones: Vec<DroneProfile>,
    reservations: Mutex<HashMap<DroneId, ShipmentId>>,
}

impl StubFleetGateway {
    pub fn new() -> Self {
        let drone = |n: u128, max_payload_grams, max_range_m| DroneProfile {
            id: DroneId::from_uuid(Uuid::from_u128(n)),
            max_payload_grams,
            max_range_m,
        };
        Self {
            drones: vec![
                drone(1, 5_000, 20_000),
                drone(2, 3_000, 10_000),
                drone(3, 2_000, 8_000),
            ],
            reservations: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for StubFleetGateway {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FleetGateway for StubFleetGateway {
    async fn find_available_drones(
        &self,
        requirements: &DroneRequirements,
        limit: usize,
    ) -> Result<Vec<DroneProfile>, GatewayError> {
        let reservations = self.reservations.lock().unwrap();
        let mut free: Vec<_> = self
            .drones
            .iter()
            .filter(|d| !reservations.contains_key(&d.id))
            .filter(|d| d.max_payload_grams >= requirements.payload_grams)
            .copied()
            .collect();
        // Best fit first: keep the strongest drones for the heaviest parcels.
        free.sort_by_key(|d| d.max_payload_grams);
        free.truncate(limit);
        Ok(free)
    }

    async fn reserve_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        if !self.drones.iter().any(|d| d.id == drone_id) {
            return Err(GatewayError::Rejected {
                service: FLEET,
                reason: format!("unknown drone {drone_id}"),
            });
        }

        let mut reservations = self.reservations.lock().unwrap();
        match reservations.get(&drone_id) {
            Some(owner) if *owner != shipment_id => Err(GatewayError::Rejected {
                service: FLEET,
                reason: format!("drone {drone_id} is already reserved"),
            }),
            _ => {
                reservations.insert(drone_id, shipment_id);
                Ok(())
            }
        }
    }

    async fn release_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        let mut reservations = self.reservations.lock().unwrap();
        if reservations.get(&drone_id) == Some(&shipment_id) {
            reservations.remove(&drone_id);
        }
        Ok(())
    }
}

/// Straight-line flights at constant speed.
#[derive(Default)]
pub struct StubRoutePlanner;

const APPROACH_DISTANCE_M: u32 = 600;
const CRUISE_SPEED_MPS: u32 = 12;
const TAKEOFF_AND_LANDING_SECS: u32 = 90;

#[async_trait]
impl RoutePlanner for StubRoutePlanner {
    async fn plan_route(
        &self,
        _drone_id: DroneId,
        pickup: &GeoPoint,
        dropoff: &GeoPoint,
    ) -> Result<RoutePlan, GatewayError> {
        let distance_m = APPROACH_DISTANCE_M + pickup.distance_to(dropoff);
        let duration_secs = distance_m / CRUISE_SPEED_MPS + TAKEOFF_AND_LANDING_SECS;
        RoutePlan::new(distance_m, duration_secs).map_err(|e| GatewayError::Rejected {
            service: PLANNER,
            reason: e.to_string(),
        })
    }
}
