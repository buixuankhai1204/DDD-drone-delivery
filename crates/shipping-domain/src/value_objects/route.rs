use crate::error::DomainError;

/// Outcome of flight planning for one delivery, as provided by the Flight
/// Planning context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePlan {
    distance_m: u32,
    duration_secs: u32,
}

impl RoutePlan {
    /// * `distance_m` – total distance the drone has to fly to complete the
    ///   delivery (current position → pickup → dropoff).
    /// * `duration_secs` – time until the parcel is expected at the dropoff.
    pub fn new(distance_m: u32, duration_secs: u32) -> Result<Self, DomainError> {
        if distance_m == 0 || duration_secs == 0 {
            return Err(DomainError::InvalidRoutePlan);
        }
        Ok(Self {
            distance_m,
            duration_secs,
        })
    }

    pub const fn distance_m(&self) -> u32 {
        self.distance_m
    }

    pub const fn duration_secs(&self) -> u32 {
        self.duration_secs
    }
}
