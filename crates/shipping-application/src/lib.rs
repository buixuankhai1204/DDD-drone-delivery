//! Application layer of the **Shipping** bounded context.
//!
//! One struct per use case (`execute` method), orchestrating the domain model
//! and the ports it needs. Ports to other bounded contexts and to messaging
//! live in [`ports`]; infrastructure provides the adapters.

pub mod dto;
pub mod error;
pub mod ports;
pub mod usecases;

pub use error::ApplicationError;
