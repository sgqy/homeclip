//! Homeclip's single-process HTTP server.

mod app;
mod db;
mod error;
mod events;
mod files;
mod messages;
mod state;

#[cfg(test)]
mod tests;

use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context;
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
    let directory = PathBuf::from(env::var("HOMECLIP_DATA").unwrap_or_else(|_| "data".into()));
    let web_root = PathBuf::from(env::var("HOMECLIP_WEB").unwrap_or_else(|_| "web/dist".into()));
    let state = state::AppState::open(&directory)?;
    let app = app::router(state.clone(), &web_root);
    let janitor = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(error) = files::collect_garbage(&state).await {
                tracing::warn!(?error, "attachment cleanup interrupted");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "homeclip is listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    janitor.abort();
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => tracing::error!(%error, "could not listen for SIGTERM"),
            }
        };
        tokio::select! { () = ctrl_c => {}, () = terminate => {} }
    }
    #[cfg(not(unix))]
    ctrl_c.await;
}
