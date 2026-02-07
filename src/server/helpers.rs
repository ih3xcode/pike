use crate::types::AppState;

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

pub fn resolve_base_url(state: &AppState, host: &Option<String>) -> String {
    if state.public_url.is_some() {
        return state.base_url();
    }
    match host {
        Some(h) => state.base_url_with_host(h),
        None => state.base_url(),
    }
}

// URL construction — extracted from types.rs (belongs to server layer)
impl AppState {
    pub fn base_url_with_host(&self, host: &str) -> String {
        match &self.token {
            Some(t) => format!("http://{}/{}", host, t),
            None => format!("http://{}", host),
        }
    }

    pub fn base_url(&self) -> String {
        if let Some(url) = &self.public_url {
            let base = url.trim_end_matches('/');
            match &self.token {
                Some(t) => format!("{}/{}", base, t),
                None => base.to_string(),
            }
        } else {
            match &self.token {
                Some(t) => format!("http://{}:{}/{}", self.addr, self.port, t),
                None => format!("http://{}:{}", self.addr, self.port),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicU32, Arc, Mutex};
    use tokio::sync::{Notify, RwLock};

    fn test_state(token: Option<&str>, public_url: Option<&str>) -> AppState {
        AppState {
            token: token.map(|s| s.to_string()),
            cid: String::new(),
            cloud: None,
            addr: "10.0.0.1".into(),
            port: 8080,
            public_url: public_url.map(|s| s.to_string()),
            sensors: RwLock::new(vec![]),
            download_count: AtomicU32::new(0),
            max_downloads: 0,
            shutdown_notify: Arc::new(Notify::new()),
            falcon_client: None,
            hosts: Mutex::new(vec![]),
        }
    }

    // --- base_url ---

    #[test]
    fn base_url_no_token_no_public() {
        let state = test_state(None, None);
        assert_eq!(state.base_url(), "http://10.0.0.1:8080");
    }

    #[test]
    fn base_url_with_token() {
        let state = test_state(Some("abc123"), None);
        assert_eq!(state.base_url(), "http://10.0.0.1:8080/abc123");
    }

    #[test]
    fn base_url_with_public_url() {
        let state = test_state(None, Some("https://deploy.example.com"));
        assert_eq!(state.base_url(), "https://deploy.example.com");
    }

    #[test]
    fn base_url_public_url_with_token() {
        let state = test_state(Some("tok"), Some("https://deploy.example.com/"));
        assert_eq!(state.base_url(), "https://deploy.example.com/tok");
    }

    // --- base_url_with_host ---

    #[test]
    fn base_url_with_host_no_token() {
        let state = test_state(None, None);
        assert_eq!(state.base_url_with_host("myhost:9090"), "http://myhost:9090");
    }

    #[test]
    fn base_url_with_host_and_token() {
        let state = test_state(Some("t0k"), None);
        assert_eq!(state.base_url_with_host("myhost:9090"), "http://myhost:9090/t0k");
    }
}
