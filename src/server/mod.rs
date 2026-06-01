//! Web layer: the workbench view and JSON API, both served by this binary.
//!
//! A small HTTP server serves the static frontend (embedded in the binary) and
//! exposes the API that connects the UI to the forge in `engine`.

pub mod routes;
pub mod state;

use std::net::SocketAddr;

use anyhow::Result;

/// Build the router and start serving the workbench on `127.0.0.1:port`.
pub async fn run(port: u16) -> Result<()> {
    let app = routes::router(state::AppState::default());
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    tracing::info!("mcpscrk workbench listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
