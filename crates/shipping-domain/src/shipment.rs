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
