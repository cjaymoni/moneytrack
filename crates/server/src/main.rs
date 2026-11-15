//! The binary: read the environment, migrate, listen.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use mt_server::config::Config;
use mt_server::limit::Attempts;
use mt_server::{AppState, db, router};
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mt_server=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url)?;
    db::migrate(&pool).await?;

    let bind = config.bind.clone();
    let state = AppState {
        pool,
        config: Arc::new(config),
        attempts: Arc::new(Attempts::default()),
    };

    let listener = TcpListener::bind(&bind).await?;
    tracing::info!(address = %listener.local_addr()?, "listening");

    // `ConnectInfo` is what the rate limiter reads the caller's address from. Behind a proxy
    // that address is the proxy's, and the limiter will need the forwarded header instead —
    // which is only safe once there is a proxy we trust, so it is not read here yet.
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

/// Finishes in-flight requests when the platform asks the process to stop.
async fn shutdown() {
    let interrupt = async { signal::ctrl_c().await.ok() };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => stream.recv().await,
            Err(error) => {
                tracing::error!(%error, "could not listen for SIGTERM");
                None
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<Option<()>>();

    tokio::select! {
        _ = interrupt => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
