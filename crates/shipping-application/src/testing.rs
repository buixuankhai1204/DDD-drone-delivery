//! In-memory test doubles for the application layer's ports.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use shipping_domain::{
    DroneId, DroneProfile, GeoPoint, Page, Paged, RepositoryError, RoutePlan, Shipment,
    ShipmentFilter, ShipmentId, ShipmentRepository, TrackingNumber,
};

use crate::ports::{DroneRequirements, FleetGateway, GatewayError, RoutePlanner};

#[derive(Default)]
pub struct InMemoryShipments {
    store: Mutex<HashMap<ShipmentId, Shipment>>,
    fail_next_save: AtomicBool,
}

impl InMemoryShipments {
    /// Makes the next `save` fail with an optimistic-locking conflict.
    pub fn fail_next_save(&self) {
        self.fail_next_save.store(true, Ordering::SeqCst);
    }

    pub fn get(&self, id: ShipmentId) -> Option<Shipment> {
        self.store.lock().unwrap().get(&id).cloned()
    }
}

#[async_trait]
impl ShipmentRepository for InMemoryShipments {
    async fn find_by_id(&self, id: ShipmentId) -> Result<Option<Shipment>, RepositoryError> {
        Ok(self.get(id))
    }

    async fn find_by_tracking_number(
        &self,
        tracking_number: &TrackingNumber,
    ) -> Result<Option<Shipment>, RepositoryError> {
        let store = self.store.lock().unwrap();
        Ok(store
            .values()
            .find(|s| s.tracking_number() == tracking_number)
            .cloned())
    }

    async fn search(
        &self,
        filter: ShipmentFilter,
        page: Page,
    ) -> Result<Paged<Shipment>, RepositoryError> {
        let store = self.store.lock().unwrap();
        let mut matching: Vec<_> = store
            .values()
            .filter(|s| filter.customer_id.is_none_or(|c| c == s.customer_id()))
            .filter(|s| filter.status.is_none_or(|st| st == s.status()))
            .cloned()
            .collect();
        matching.sort_by_key(|s| std::cmp::Reverse(s.requested_at()));

        let total = matching.len() as u64;
        let items = matching
            .into_iter()
            .skip(page.offset as usize)
            .take(page.limit as usize)
            .collect();
        Ok(Paged { items, total })
    }

    async fn save(&self, shipment: &mut Shipment) -> Result<(), RepositoryError> {
        if self.fail_next_save.swap(false, Ordering::SeqCst) {
            return Err(RepositoryError::Conflict(shipment.id()));
        }

        let mut store = self.store.lock().unwrap();
        let stored_version = store.get(&shipment.id()).map(Shipment::version);
        let consistent = match stored_version {
            None => shipment.is_new(),
            Some(version) => version == shipment.version(),
        };
        if !consistent {
            return Err(RepositoryError::Conflict(shipment.id()));
        }

        shipment.mark_persisted();
        store.insert(shipment.id(), shipment.clone());
        Ok(())
    }
}

pub struct FakeFleet {
    drones: Vec<DroneProfile>,
    reserved: Mutex<HashMap<DroneId, ShipmentId>>,
    reject_reservation_for: Mutex<HashSet<DroneId>>,
    released: Mutex<Vec<DroneId>>,
}

impl FakeFleet {
    pub fn with_drones(drones: Vec<DroneProfile>) -> Self {
        Self {
            drones,
            reserved: Mutex::default(),
            reject_reservation_for: Mutex::default(),
            released: Mutex::default(),
        }
    }

    /// Simulates another dispatcher grabbing the drone first.
    pub fn reject_reservation_for(&self, drone: DroneId) {
        self.reject_reservation_for.lock().unwrap().insert(drone);
    }

    pub fn reserved(&self) -> HashMap<DroneId, ShipmentId> {
        self.reserved.lock().unwrap().clone()
    }

    pub fn released(&self) -> Vec<DroneId> {
        self.released.lock().unwrap().clone()
    }
}

#[async_trait]
impl FleetGateway for FakeFleet {
    async fn find_available_drones(
        &self,
        _requirements: &DroneRequirements,
        limit: usize,
    ) -> Result<Vec<DroneProfile>, GatewayError> {
        let reserved = self.reserved.lock().unwrap();
        Ok(self
            .drones
            .iter()
            .filter(|d| !reserved.contains_key(&d.id))
            .take(limit)
            .copied()
            .collect())
    }

    async fn reserve_drone(
        &self,
        drone_id: DroneId,
        shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        if self
            .reject_reservation_for
            .lock()
            .unwrap()
            .contains(&drone_id)
        {
            return Err(GatewayError::Rejected {
                service: "fleet",
                reason: "drone already reserved".into(),
            });
        }
        self.reserved.lock().unwrap().insert(drone_id, shipment_id);
        Ok(())
    }

    async fn release_drone(
        &self,
        drone_id: DroneId,
        _shipment_id: ShipmentId,
    ) -> Result<(), GatewayError> {
        self.reserved.lock().unwrap().remove(&drone_id);
        self.released.lock().unwrap().push(drone_id);
        Ok(())
    }
}

pub struct FakePlanner {
    route: Result<RoutePlan, String>,
}

impl FakePlanner {
    pub fn flying(distance_m: u32, duration_secs: u32) -> Self {
        Self {
            route: Ok(RoutePlan::new(distance_m, duration_secs).unwrap()),
        }
    }

    pub fn no_route(reason: &str) -> Self {
        Self {
            route: Err(reason.to_owned()),
        }
    }
}

#[async_trait]
impl RoutePlanner for FakePlanner {
    async fn plan_route(
        &self,
        _drone_id: DroneId,
        _pickup: &GeoPoint,
        _dropoff: &GeoPoint,
    ) -> Result<RoutePlan, GatewayError> {
        self.route.clone().map_err(|reason| GatewayError::Rejected {
            service: "flight-planning",
            reason,
        })
    }
}
