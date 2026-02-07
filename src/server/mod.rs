mod handlers;
mod helpers;

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::sync::Notify;

use crate::types::AppState;

use handlers::*;

pub fn router(state: Arc<AppState>) -> Router {
    let routes = Router::new()
        .route("/lin", get(install_sh))
        .route("/win", get(install_ps1))
        .route("/s/{filename}", get(sensor_download))
        .route("/cb", post(callback))
        .route("/done", post(done));

    if let Some(ref token) = state.token {
        eprintln!("[router] Routes: /{token}/lin, /{token}/win, /{token}/s/*, /{token}/cb, /{token}/done");
        Router::new()
            .nest(&format!("/{token}"), routes)
            .with_state(state)
    } else {
        eprintln!("[router] Routes: /lin, /win, /s/*, /cb, /done (no auth)");
        routes.with_state(state)
    }
}

pub async fn run_server(
    state: Arc<AppState>,
    bind_addr: std::net::SocketAddr,
    timeout: u64,
    shutdown_notify: Arc<Notify>,
    handle_ctrlc: bool,
) -> Result<(), String> {
    let app = router(state);

    eprintln!("[server] Binding to {bind_addr} ...");
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Cannot bind to {bind_addr}: {e}"))?;
    eprintln!("[server] Listening on {bind_addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(crate::shutdown::shutdown_signal(
        timeout,
        shutdown_notify,
        handle_ctrlc,
    ))
    .await
    .map_err(|e| format!("Server error: {e}"))?;

    eprintln!("[server] Server stopped");
    Ok(())
}
