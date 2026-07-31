use axum::{
    extract::{OriginalUri, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::scripts;
use crate::server::helpers::{log_request, peer_addr};
use crate::server::state::AppState;

pub async fn install_sh(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    serve_script(&state, &remote, uri.path(), Platform::Linux)
}

pub async fn install_ps1(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    serve_script(&state, &remote, uri.path(), Platform::Windows)
}

#[derive(Clone, Copy)]
enum Platform {
    Linux,
    Windows,
}

impl Platform {
    fn label(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::Windows => "windows",
        }
    }
}

fn serve_script(state: &AppState, remote: &str, path: &str, platform: Platform) -> Response {
    if state.cid.is_empty() {
        log_request("GET", path, remote, 404, "no CID configured");
        return StatusCode::NOT_FOUND.into_response();
    }

    // The URL is baked into a script the far end runs as root, so it comes
    // from the configuration only. Deriving it from the Host header let a
    // single poisoned request through a caching proxy — or any redirector —
    // point every later victim at an attacker's binary. Reverse-proxy setups
    // say where they live with --public-url.
    let base_url = state.base_url();
    let script = match platform {
        Platform::Linux => scripts::generate_linux_script(
            &base_url,
            &state.cid,
            state.cloud.as_deref(),
            state.tags.as_deref(),
        ),
        Platform::Windows => scripts::generate_windows_script(
            &base_url,
            &state.cid,
            state.cloud.as_deref(),
            state.tags.as_deref(),
        ),
    };

    let label = platform.label();
    log_request("GET", path, remote, 200, &format!("{label} script"));
    eprintln!("[script] Served {label} install script to {remote} (base_url={base_url})");

    let mut resp = script.into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}
