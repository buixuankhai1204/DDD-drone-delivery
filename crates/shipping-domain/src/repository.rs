use async_trait::async_trait;

use crate::{
    shipment::Shipment,
    status::ShipmentStatus,
    value_objects::{CustomerId, ShipmentId, TrackingNumber},
};

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    /// The aggregate was modified (or created) concurrently by someone else.
    #[error("shipment {0} was modified concurrently")]
    Conflict(ShipmentId),

    #[error("storage failure: {0}")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl RepositoryError {
    pub fn storage(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(error))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShipmentFilter {
    pub customer_id: Option<CustomerId>,
    pub status: Option<ShipmentStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paged<T> {
    pub items: Vec<T>,
    /// Number of items matching the filter, ignoring pagination.
    pub total: u64,
}

/// Persistence port of the [`Shipment`] aggregate.
///
/// The domain owns the interface, infrastructure implements it.
#[async_trait]
pub trait ShipmentRepository: Send + Sync {
    async fn find_by_id(&self, id: ShipmentId) -> Result<Option<Shipment>, RepositoryError>;

    async fn find_by_tracking_number(
        &self,
        tracking_number: &TrackingNumber,
    ) -> Result<Option<Shipment>, RepositoryError>;

    async fn search(
        &self,
        filter: ShipmentFilter,
        page: Page,
    ) -> Result<Paged<Shipment>, RepositoryError>;

    /// Inserts a new aggregate or updates an existing one using optimistic
    /// locking on its version.
    ///
    /// The state change and the aggregate's pending domain events are stored
    /// atomically (transactional outbox). On success the aggregate is marked
    /// as persisted; on [`RepositoryError::Conflict`] it is left untouched.
    async fn save(&self, shipment: &mut Shipment) -> Result<(), RepositoryError>;
}
