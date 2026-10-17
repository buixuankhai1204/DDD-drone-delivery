//! Interface layer of the **Shipping** bounded context: the REST API.
//!
//! Translates HTTP into use-case calls and use-case results/errors back into
//! HTTP. No business rules live here.

pub mod config;
mod error;
mod extract;
mod routes;
pub mod state;

pub use routes::router;
pub use state::AppState;
