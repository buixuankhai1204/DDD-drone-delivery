//! Infrastructure layer of the **Shipping** bounded context: adapters that
//! implement the ports defined by the domain and application layers.
//!
//! * [`persistence`] – PostgreSQL implementation of the shipment repository
//! * [`gateways`] – HTTP clients (and local stubs) for other bounded contexts
//! * [`messaging`] – transactional-outbox relay and event publishers
//! * [`database`] – pool and migrations

pub mod database;
pub mod gateways;
pub mod messaging;
pub mod persistence;
