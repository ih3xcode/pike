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


