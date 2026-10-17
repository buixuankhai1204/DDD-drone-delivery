use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    error::DomainError,
    events::ShipmentEvent,
    services::dispatch_policy::DispatchPolicy,
    status::ShipmentStatus,
    value_objects::{
        CustomerId, DroneAssignment, DroneId, DroneProfile, Location, Parcel, Recipient, RoutePlan,
        ShipmentId, TrackingNumber,
    },
};

/// Longest straight-line distance between pickup and dropoff we serve.
pub const MAX_SERVICE_DISTANCE_M: u32 = 25_000;
/// Below this distance a drone delivery makes no sense.
pub const MIN_SERVICE_DISTANCE_M: u32 = 50;

/// One entry of the customer-visible tracking timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackingEntry {
    pub status: ShipmentStatus,
    pub occurred_at: DateTime<Utc>,
    pub note: Option<String>,
}

/// Data needed to request a new shipment.
#[derive(Debug, Clone)]
pub struct NewShipment {
    pub customer_id: CustomerId,
    pub recipient: Recipient,
    pub parcel: Parcel,
    pub pickup: Location,
    pub dropoff: Location,
}

/// Plain state of a persisted shipment, used by repositories to rebuild the
/// aggregate. Not a way to bypass invariants for new shipments: use
/// [`Shipment::request`] for that.
#[derive(Debug, Clone)]
pub struct ShipmentParts {
    pub id: ShipmentId,
    pub tracking_number: TrackingNumber,
    pub customer_id: CustomerId,
    pub recipient: Recipient,
    pub parcel: Parcel,
    pub pickup: Location,
    pub dropoff: Location,
    pub status: ShipmentStatus,
    pub assignment: Option<DroneAssignment>,
    pub route: Option<RoutePlan>,
    pub estimated_delivery_at: Option<DateTime<Utc>>,
    pub history: Vec<TrackingEntry>,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

/// Aggregate root of the Shipping context.
///
/// All state changes go through the methods below, which enforce the
/// lifecycle rules and record the matching [`ShipmentEvent`]s.
#[derive(Debug, Clone)]
pub struct Shipment {
    id: ShipmentId,
    tracking_number: TrackingNumber,
    customer_id: CustomerId,
    recipient: Recipient,
    parcel: Parcel,
    pickup: Location,
    dropoff: Location,
    status: ShipmentStatus,
    assignment: Option<DroneAssignment>,
    route: Option<RoutePlan>,
    estimated_delivery_at: Option<DateTime<Utc>>,
    history: Vec<TrackingEntry>,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// Optimistic-locking version; `0` means "never persisted".
    version: i64,
    events: Vec<ShipmentEvent>,
}

impl Shipment {
    /// Creates a new shipment in the [`ShipmentStatus::Requested`] state.
    pub fn request(new: NewShipment, now: DateTime<Utc>) -> Result<Self, DomainError> {
        let distance_m = new
            .pickup
            .coordinates()
            .distance_to(new.dropoff.coordinates());
        if distance_m < MIN_SERVICE_DISTANCE_M {
            return Err(DomainError::LocationsTooClose {
                distance_m,
                min_m: MIN_SERVICE_DISTANCE_M,
            });
        }
        if distance_m > MAX_SERVICE_DISTANCE_M {
            return Err(DomainError::OutsideServiceArea {
                distance_m,
                max_m: MAX_SERVICE_DISTANCE_M,
            });
        }

        let id = ShipmentId::new();
        let tracking_number = TrackingNumber::generate();
        let event = ShipmentEvent::Requested {
            shipment_id: id,
            tracking_number: tracking_number.clone(),
            customer_id: new.customer_id,
            occurred_at: now,
        };

        Ok(Self {
            id,
            tracking_number,
            customer_id: new.customer_id,
            recipient: new.recipient,
            parcel: new.parcel,
            pickup: new.pickup,
            dropoff: new.dropoff,
            status: ShipmentStatus::Requested,
            assignment: None,
            route: None,
            estimated_delivery_at: None,
            history: vec![TrackingEntry {
                status: ShipmentStatus::Requested,
                occurred_at: now,
                note: None,
            }],
            requested_at: now,
            updated_at: now,
            version: 0,
            events: vec![event],
        })
    }

    /// Rebuilds a persisted shipment. Records no events.
    pub fn restore(parts: ShipmentParts) -> Self {
        Self {
            id: parts.id,
            tracking_number: parts.tracking_number,
            customer_id: parts.customer_id,
            recipient: parts.recipient,
            parcel: parts.parcel,
            pickup: parts.pickup,
            dropoff: parts.dropoff,
            status: parts.status,
            assignment: parts.assignment,
            route: parts.route,
            estimated_delivery_at: parts.estimated_delivery_at,
            history: parts.history,
            requested_at: parts.requested_at,
            updated_at: parts.updated_at,
            version: parts.version,
            events: Vec::new(),
        }
    }

    // ---- behaviour -------------------------------------------------------

    /// Fails unless the shipment is waiting for a drone.
    pub fn ensure_awaiting_dispatch(&self) -> Result<(), DomainError> {
        self.guard("dispatch", &[ShipmentStatus::Requested])
    }

    /// Checks that `drone` may fly this shipment along `route`, without
    /// changing anything. Lets callers fail early before reserving the drone.
    pub fn ensure_can_schedule(
        &self,
        drone: &DroneProfile,
        route: &RoutePlan,
    ) -> Result<(), DomainError> {
        self.ensure_awaiting_dispatch()?;
        DispatchPolicy::verify(&self.parcel, route, drone)
    }

    /// Assigns a drone and a planned route. `Requested → Scheduled`.
    pub fn schedule(
        &mut self,
        drone: &DroneProfile,
        route: RoutePlan,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.ensure_can_schedule(drone, &route)?;

        let estimated_delivery_at = now + Duration::seconds(i64::from(route.duration_secs()));
        self.assignment = Some(DroneAssignment {
            drone_id: drone.id,
            assigned_at: now,
        });
        self.route = Some(route);
        self.estimated_delivery_at = Some(estimated_delivery_at);
        self.move_to(
            ShipmentStatus::Scheduled,
            Some(format!("Drone {} assigned", drone.id)),
            now,
        );
        self.events.push(ShipmentEvent::Scheduled {
            shipment_id: self.id,
            drone_id: drone.id,
            estimated_delivery_at,
            occurred_at: now,
        });
        Ok(())
    }

    /// The drone collected the parcel. `Scheduled → InTransit`.
    pub fn record_pickup(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.guard("pick up", &[ShipmentStatus::Scheduled])?;
        let drone_id = self.require_drone()?;

        self.move_to(ShipmentStatus::InTransit, None, now);
        self.events.push(ShipmentEvent::PickedUp {
            shipment_id: self.id,
            drone_id,
            occurred_at: now,
        });
        Ok(())
    }

    /// The parcel reached the dropoff point. `InTransit → Delivered`.
    pub fn record_delivery(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.guard("deliver", &[ShipmentStatus::InTransit])?;
        let drone_id = self.require_drone()?;

        self.move_to(ShipmentStatus::Delivered, None, now);
        self.events.push(ShipmentEvent::Delivered {
            shipment_id: self.id,
            drone_id,
            occurred_at: now,
        });
        Ok(())
    }

    /// The delivery attempt failed after pickup. `InTransit → Failed`.
    pub fn record_delivery_failure(
        &mut self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.guard("fail", &[ShipmentStatus::InTransit])?;
        let drone_id = self.require_drone()?;
        let reason = crate::value_objects::non_blank("reason", reason, 500)?;

        self.move_to(ShipmentStatus::Failed, Some(reason.clone()), now);
        self.events.push(ShipmentEvent::DeliveryFailed {
            shipment_id: self.id,
            drone_id,
            reason,
            occurred_at: now,
        });
        Ok(())
    }

    /// Customer or operator cancels before the drone took off with the
    /// parcel. `Requested | Scheduled → Cancelled`.
    pub fn cancel(&mut self, reason: Option<&str>, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.guard(
            "cancel",
            &[ShipmentStatus::Requested, ShipmentStatus::Scheduled],
        )?;
        let reason = reason
            .map(|r| crate::value_objects::non_blank("reason", r, 500))
            .transpose()?;

        self.move_to(ShipmentStatus::Cancelled, reason.clone(), now);
        self.events.push(ShipmentEvent::Cancelled {
            shipment_id: self.id,
            drone_id: self.assigned_drone(),
            reason,
            occurred_at: now,
        });
        Ok(())
    }

    // ---- persistence hooks ----------------------------------------------

    pub const fn is_new(&self) -> bool {
        self.version == 0
    }

    /// Domain events recorded since the aggregate was loaded or created.
    pub fn pending_events(&self) -> &[ShipmentEvent] {
        &self.events
    }

    /// To be called by the repository once state and events are stored.
    pub fn mark_persisted(&mut self) {
        self.events.clear();
        self.version += 1;
    }

    // ---- accessors -------------------------------------------------------

    pub const fn id(&self) -> ShipmentId {
        self.id
    }

    pub const fn tracking_number(&self) -> &TrackingNumber {
        &self.tracking_number
    }

    pub const fn customer_id(&self) -> CustomerId {
        self.customer_id
    }

    pub const fn recipient(&self) -> &Recipient {
        &self.recipient
    }

    pub const fn parcel(&self) -> &Parcel {
        &self.parcel
    }

    pub const fn pickup(&self) -> &Location {
        &self.pickup
    }

    pub const fn dropoff(&self) -> &Location {
        &self.dropoff
    }

    pub const fn status(&self) -> ShipmentStatus {
        self.status
    }

    pub const fn assignment(&self) -> Option<&DroneAssignment> {
        self.assignment.as_ref()
    }

    pub fn assigned_drone(&self) -> Option<DroneId> {
        self.assignment.map(|a| a.drone_id)
    }

    pub const fn route(&self) -> Option<&RoutePlan> {
        self.route.as_ref()
    }

    pub const fn estimated_delivery_at(&self) -> Option<DateTime<Utc>> {
        self.estimated_delivery_at
    }

    pub fn history(&self) -> &[TrackingEntry] {
        &self.history
    }

    pub const fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }

    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub const fn version(&self) -> i64 {
        self.version
    }

    // ---- internals -------------------------------------------------------

    fn guard(&self, action: &'static str, allowed: &[ShipmentStatus]) -> Result<(), DomainError> {
        if allowed.contains(&self.status) {
            Ok(())
        } else {
            Err(DomainError::InvalidStateTransition {
                action,
                status: self.status,
            })
        }
    }

    fn require_drone(&self) -> Result<DroneId, DomainError> {
        self.assigned_drone().ok_or(DomainError::NoDroneAssigned)
    }

    fn move_to(&mut self, status: ShipmentStatus, note: Option<String>, now: DateTime<Utc>) {
        self.status = status;
        self.updated_at = now;
        self.history.push(TrackingEntry {
            status,
            occurred_at: now,
            note,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::{Dimensions, GeoPoint};

    fn location(lat: f64, lon: f64, address: &str) -> Location {
        Location::new(GeoPoint::new(lat, lon).unwrap(), address).unwrap()
    }

    fn new_shipment() -> NewShipment {
        NewShipment {
            customer_id: CustomerId::new(),
            recipient: Recipient::new("Nguyen Van A", "+84901234567").unwrap(),
            parcel: Parcel::new(
                1_500,
                Dimensions::new(200, 150, 100).unwrap(),
                "Phone case",
                true,
            )
            .unwrap(),
            // ~2.3 km apart
            pickup: location(21.0285, 105.8542, "Hoan Kiem, Hanoi"),
            dropoff: location(21.0380, 105.8340, "Ba Dinh, Hanoi"),
        }
    }

    fn drone() -> DroneProfile {
        DroneProfile {
            id: DroneId::new(),
            max_payload_grams: 3_000,
            max_range_m: 10_000,
        }
    }

    fn route() -> RoutePlan {
        RoutePlan::new(3_200, 420).unwrap()
    }

    fn requested() -> Shipment {
        Shipment::request(new_shipment(), Utc::now()).unwrap()
    }

    fn in_transit() -> Shipment {
        let mut s = requested();
        s.schedule(&drone(), route(), Utc::now()).unwrap();
        s.record_pickup(Utc::now()).unwrap();
        s
    }

    #[test]
    fn new_shipment_is_requested_and_records_event() {
        let s = requested();
        assert_eq!(s.status(), ShipmentStatus::Requested);
        assert!(s.is_new());
        assert_eq!(s.history().len(), 1);
        assert!(matches!(
            s.pending_events(),
            [ShipmentEvent::Requested { .. }]
        ));
    }

    #[test]
    fn rejects_pickup_and_dropoff_that_are_too_close_or_too_far() {
        let mut too_close = new_shipment();
        too_close.dropoff = too_close.pickup.clone();
        assert!(matches!(
            Shipment::request(too_close, Utc::now()),
            Err(DomainError::LocationsTooClose { .. })
        ));

        let mut too_far = new_shipment();
        too_far.dropoff = location(10.7769, 106.7009, "Ho Chi Minh City");
        assert!(matches!(
            Shipment::request(too_far, Utc::now()),
            Err(DomainError::OutsideServiceArea { .. })
        ));
    }

    #[test]
    fn happy_path_walks_through_every_state() {
        let now = Utc::now();
        let mut s = requested();
        let d = drone();

        s.schedule(&d, route(), now).unwrap();
        assert_eq!(s.status(), ShipmentStatus::Scheduled);
        assert_eq!(s.assigned_drone(), Some(d.id));
        assert_eq!(
            s.estimated_delivery_at(),
            Some(now + Duration::seconds(420))
        );

        s.record_pickup(now).unwrap();
        assert_eq!(s.status(), ShipmentStatus::InTransit);

        s.record_delivery(now).unwrap();
        assert_eq!(s.status(), ShipmentStatus::Delivered);
        assert!(s.status().is_terminal());

        let statuses: Vec<_> = s.history().iter().map(|e| e.status).collect();
        assert_eq!(
            statuses,
            [
                ShipmentStatus::Requested,
                ShipmentStatus::Scheduled,
                ShipmentStatus::InTransit,
                ShipmentStatus::Delivered
            ]
        );
        assert_eq!(s.pending_events().len(), 4);
    }

    #[test]
    fn scheduling_requires_a_suitable_drone() {
        let mut s = requested();
        let weak = DroneProfile {
            max_payload_grams: 500,
            ..drone()
        };
        assert!(matches!(
            s.schedule(&weak, route(), Utc::now()),
            Err(DomainError::DroneCannotCarryParcel { .. })
        ));
        // nothing changed
        assert_eq!(s.status(), ShipmentStatus::Requested);
        assert_eq!(s.assigned_drone(), None);
    }

    #[test]
    fn cannot_schedule_twice() {
        let mut s = requested();
        s.schedule(&drone(), route(), Utc::now()).unwrap();
        assert!(matches!(
            s.schedule(&drone(), route(), Utc::now()),
            Err(DomainError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn cannot_pick_up_before_scheduling() {
        let mut s = requested();
        assert!(matches!(
            s.record_pickup(Utc::now()),
            Err(DomainError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn cancel_is_allowed_before_pickup_only() {
        let mut before = requested();
        before.cancel(Some("changed my mind"), Utc::now()).unwrap();
        assert_eq!(before.status(), ShipmentStatus::Cancelled);

        let mut scheduled = requested();
        let d = drone();
        scheduled.schedule(&d, route(), Utc::now()).unwrap();
        scheduled.cancel(None, Utc::now()).unwrap();
        assert!(matches!(
            scheduled.pending_events().last(),
            Some(ShipmentEvent::Cancelled { drone_id: Some(id), .. }) if *id == d.id
        ));

        let mut flying = in_transit();
        assert!(matches!(
            flying.cancel(None, Utc::now()),
            Err(DomainError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn terminal_states_are_final() {
        let mut cancelled = requested();
        cancelled.cancel(None, Utc::now()).unwrap();
        assert!(cancelled.cancel(None, Utc::now()).is_err());
        assert!(cancelled.schedule(&drone(), route(), Utc::now()).is_err());

        let mut delivered = in_transit();
        delivered.record_delivery(Utc::now()).unwrap();
        assert!(delivered.record_delivery(Utc::now()).is_err());
        assert!(delivered.record_delivery_failure("x", Utc::now()).is_err());
    }

    #[test]
    fn delivery_failure_requires_a_reason_and_pickup() {
        let mut s = in_transit();
        assert_eq!(
            s.record_delivery_failure("  ", Utc::now()).unwrap_err(),
            DomainError::BlankField("reason")
        );
        s.record_delivery_failure("Recipient unreachable", Utc::now())
            .unwrap();
        assert_eq!(s.status(), ShipmentStatus::Failed);

        let mut not_flying = requested();
        assert!(not_flying.record_delivery_failure("x", Utc::now()).is_err());
    }

    #[test]
    fn mark_persisted_clears_events_and_bumps_version() {
        let mut s = requested();
        s.mark_persisted();
        assert!(!s.is_new());
        assert_eq!(s.version(), 1);
        assert!(s.pending_events().is_empty());
    }
}
