//! Homeclip's single-process HTTP server.

use std::{env, net::SocketAddr};

use anyhow::Context;
use axum::{Router, routing::get};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "homeclip=info".into()),
        )
        .init();
    let address: SocketAddr = env::var("HOMECLIP_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("parsing HOMECLIP_BIND")?;
    let app = Router::new().route("/api/health", get(|| async { "ok" }));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "homeclip is listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
