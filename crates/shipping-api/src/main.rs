use std::sync::Arc;

use anyhow::{Context, Result};
use shipping_api::{
    AppState,
    config::{Config, ExternalServices},
    router,
};
use shipping_application::ports::{FleetGateway, RoutePlanner};
use shipping_infrastructure::{
    database,
    gateways::{
        HttpFleetGateway, HttpRoutePlanner, StubFleetGateway, StubRoutePlanner, http_client,
    },
    messaging::{LoggingEventPublisher, OutboxRelay},
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // A missing .env file is fine: real environment variables win anyway.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info")),
        )
        .init();

    let config = Config::from_env()?;

    let pool = database::connect(&config.database_url, config.db_max_connections)
        .await
        .context("cannot connect to PostgreSQL")?;
    database::migrate(&pool)
        .await
        .context("database migration failed")?;

    let (fleet, routes): (Arc<dyn FleetGateway>, Arc<dyn RoutePlanner>) =
        match &config.external_services {
            ExternalServices::Http {
                fleet_url,
                flight_planning_url,
            } => {
                tracing::info!(%fleet_url, %flight_planning_url, "using HTTP gateways");
                let client = http_client();
                (
                    Arc::new(HttpFleetGateway::new(client.clone(), fleet_url)),
                    Arc::new(HttpRoutePlanner::new(client, flight_planning_url)),
                )
            }
            ExternalServices::Stub => {
                tracing::warn!("using in-process STUB gateways for fleet and flight planning");
                (
                    Arc::new(StubFleetGateway::new()),
                    Arc::new(StubRoutePlanner),
                )
            }
        };

    let shutdown = CancellationToken::new();
    let relay = tokio::spawn(
        OutboxRelay::new(pool.clone(), Arc::new(LoggingEventPublisher)).run(shutdown.clone()),
    );

    let app = router(AppState::new(pool, fleet, routes));
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("cannot bind {}", config.bind_addr))?;
    tracing::info!(addr = %config.bind_addr, "shipping service listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    shutdown.cancel();
    relay.await.context("outbox relay panicked")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
