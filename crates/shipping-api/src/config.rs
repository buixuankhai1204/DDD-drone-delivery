use std::{env, net::SocketAddr};

use anyhow::{Context, Result, bail};

/// How the Fleet and Flight Planning contexts are reached.
#[derive(Debug, Clone)]
pub enum ExternalServices {
    /// Call the real services over HTTP.
    Http {
        fleet_url: String,
        flight_planning_url: String,
    },
    /// In-process stand-ins, for local development without the other services.
    Stub,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub db_max_connections: u32,
    pub external_services: ExternalServices,
}

impl Config {
    /// Reads the configuration from environment variables:
    ///
    /// | Variable                | Default          | Notes                                   |
    /// |-------------------------|------------------|-----------------------------------------|
    /// | `DATABASE_URL`          | –                | required                                |
    /// | `BIND_ADDR`             | `0.0.0.0:8080`   |                                         |
    /// | `DB_MAX_CONNECTIONS`    | `10`             |                                         |
    /// | `EXTERNAL_SERVICES`     | `http`           | `http` or `stub`                        |
    /// | `FLEET_SERVICE_URL`     | –                | required when `EXTERNAL_SERVICES=http`  |
    /// | `FLIGHT_PLANNING_URL`   | –                | required when `EXTERNAL_SERVICES=http`  |
    pub fn from_env() -> Result<Self> {
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
            .parse()
            .context("BIND_ADDR must be a socket address like 0.0.0.0:8080")?;

        let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;

        let db_max_connections = match env::var("DB_MAX_CONNECTIONS") {
            Ok(value) => value
                .parse()
                .context("DB_MAX_CONNECTIONS must be a positive integer")?,
            Err(_) => 10,
        };

        let external_services = match env::var("EXTERNAL_SERVICES").as_deref().unwrap_or("http") {
            "stub" => ExternalServices::Stub,
            "http" => ExternalServices::Http {
                fleet_url: env::var("FLEET_SERVICE_URL")
                    .context("FLEET_SERVICE_URL must be set when EXTERNAL_SERVICES=http")?,
                flight_planning_url: env::var("FLIGHT_PLANNING_URL")
                    .context("FLIGHT_PLANNING_URL must be set when EXTERNAL_SERVICES=http")?,
            },
            other => bail!("EXTERNAL_SERVICES must be 'http' or 'stub', got '{other}'"),
        };

        Ok(Self {
            bind_addr,
            database_url,
            db_max_connections,
            external_services,
        })
    }
}
