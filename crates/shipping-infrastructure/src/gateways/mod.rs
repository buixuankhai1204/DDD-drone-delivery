//! Adapters for the ports towards other bounded contexts.
//!
//! * [`HttpFleetGateway`] / [`HttpRoutePlanner`] talk REST to the real services.
//! * [`StubFleetGateway`] / [`StubRoutePlanner`] are in-process stand-ins for
//!   local development and demos, when those services do not exist yet.
//!
//! The wire formats below are Shipping's *assumption* of the other contexts'
//! public APIs (see the README); they are mapped to Shipping's own model right
//! here, so a change on the other side only touches this module.

mod http_fleet;
mod http_routing;
mod stub;

use std::time::Duration;

use reqwest::{Response, StatusCode};
use shipping_application::ports::GatewayError;

pub use http_fleet::HttpFleetGateway;
pub use http_routing::HttpRoutePlanner;
pub use stub::{StubFleetGateway, StubRoutePlanner};

/// Shared HTTP client: bounded timeouts so a slow neighbour cannot hang us.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .user_agent(concat!("shipping-service/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("static HTTP client configuration is valid")
}

/// Turns transport failures and non-2xx answers into [`GatewayError`]s:
/// 5xx / 408 / 429 are transient (`Unavailable`), other 4xx are refusals
/// (`Rejected`).
async fn ensure_success(
    service: &'static str,
    response: Result<Response, reqwest::Error>,
) -> Result<Response, GatewayError> {
    let response = response.map_err(|e| GatewayError::Unavailable {
        service,
        reason: e.without_url().to_string(),
    })?;

    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    let reason = format!(
        "HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    );
    let transient = status.is_server_error()
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS;

    Err(if transient {
        GatewayError::Unavailable { service, reason }
    } else {
        GatewayError::Rejected { service, reason }
    })
}

fn invalid_response(service: &'static str, error: impl std::fmt::Display) -> GatewayError {
    GatewayError::Unavailable {
        service,
        reason: format!("invalid response: {error}"),
    }
}

fn trim_base_url(base_url: impl Into<String>) -> String {
    base_url.into().trim_end_matches('/').to_owned()
}
