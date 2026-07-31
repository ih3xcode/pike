use super::state::AppState;

pub fn log_request(method: &str, path: &str, remote: &str, status: u16, extra: &str) {
    let now = chrono::Local::now().format("%H:%M:%S");
    if extra.is_empty() {
        eprintln!("[{now}] {method} {path} from {remote} — {status}");
    } else {
        eprintln!("[{now}] {method} {path} from {remote} — {status} ({extra})");
    }
}

pub fn peer_addr(req: &axum::extract::Request) -> String {
    req.extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

pub fn host_header(req: &axum::extract::Request) -> Option<String> {
    req.headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Явно налаштований `public_url` завжди перемагає Host-заголовок:
/// інакше клієнт за reverse proxy отримав би внутрішню адресу.
pub fn resolve_base_url(state: &AppState, host: &Option<String>) -> String {
    if state.public_url.is_some() {
        return state.base_url();
    }
    match host {
        Some(h) => state.base_url_with_host(h),
        None => state.base_url(),
    }
}
