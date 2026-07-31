use std::sync::{atomic::AtomicU32, Arc, Mutex};
use tokio::sync::Notify;

use crate::sensors::{BinaryStore, MetadataCache, Sensor};

const MAX_HOSTS: usize = 10_000;

#[derive(Debug, Clone)]
pub enum HostStatus {
    Registered,
    SensorReady,
    Installed,
    Failed(#[allow(dead_code)] String),
}

impl HostStatus {
    pub fn icon(&self) -> &str {
        match self {
            HostStatus::Registered => "○",
            HostStatus::SensorReady => "◐",
            HostStatus::Installed => "●",
            HostStatus::Failed(_) => "✕",
        }
    }

    pub fn text(&self) -> &str {
        match self {
            HostStatus::Registered => "Registered",
            HostStatus::SensorReady => "Ready",
            HostStatus::Installed => "Installed",
            HostStatus::Failed(_) => "Failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostEntry {
    pub hostname: String,
    pub platform: String,
    #[allow(dead_code)]
    pub arch: String,
    #[allow(dead_code)]
    pub ip: String,
    pub status: HostStatus,
    pub time: chrono::DateTime<chrono::Local>,
}

/// Shared server state. Both caches hold `Arc<dyn …>` inside, so there are
/// no type parameters here and no mention of CrowdStrike — the state can be
/// assembled from fakes right inside a test.
pub struct AppState {
    pub token: Option<String>,
    pub cid: String,
    pub cloud: Option<String>,
    pub addr: String,
    pub port: u16,
    pub public_url: Option<String>,
    /// Explicitly supplied files. Immutable after startup — anything
    /// downloaded from the API deliberately stays out, otherwise the first
    /// downloaded version would beat fresh metadata forever.
    pub local_sensors: Vec<Sensor>,
    pub metadata: Option<Arc<MetadataCache>>,
    pub store: Option<Arc<BinaryStore>>,
    pub download_count: AtomicU32,
    pub max_downloads: u32,
    pub shutdown_notify: Arc<Notify>,
    pub hosts: Mutex<Vec<HostEntry>>,
    pub tags: Option<String>,
}

#[cfg(test)]
pub const TEST_MAX_HOSTS: usize = MAX_HOSTS;

impl AppState {
    pub fn push_host(&self, entry: HostEntry) {
        let mut hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        if hosts.len() >= MAX_HOSTS {
            let drain_count = hosts.len() / 4;
            hosts.drain(..drain_count);
        }
        hosts.push(entry);
    }

    pub fn update_host_status(&self, hostname: &str, status: HostStatus) {
        let mut hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = hosts.iter_mut().rev().find(|h| h.hostname == hostname) {
            entry.status = status;
            entry.time = chrono::Local::now();
        }
    }

    /// Base URL for the one-liner, built from the request's Host header.
    pub fn base_url_with_host(&self, host: &str) -> String {
        match &self.token {
            Some(t) => format!("http://{}/{}", host, t),
            None => format!("http://{}", host),
        }
    }

    /// Base URL from the configuration: `public_url` when set, otherwise
    /// the advertised address and port.
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
pub(crate) mod test_support {
    use super::*;

    /// Minimal state for tests: no sensors, no API, no limits.
    pub(crate) fn state(token: Option<&str>, public_url: Option<&str>) -> AppState {
        AppState {
            token: token.map(|s| s.to_string()),
            cid: String::new(),
            cloud: None,
            addr: "10.0.0.1".into(),
            port: 8080,
            public_url: public_url.map(|s| s.to_string()),
            local_sensors: vec![],
            metadata: None,
            store: None,
            download_count: AtomicU32::new(0),
            max_downloads: 0,
            shutdown_notify: Arc::new(Notify::new()),
            hosts: Mutex::new(vec![]),
            tags: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::state;
    use super::*;

    fn test_host(hostname: &str) -> HostEntry {
        HostEntry {
            hostname: hostname.to_string(),
            platform: "deb".into(),
            arch: "x86_64".into(),
            ip: "10.0.0.1".into(),
            status: HostStatus::Registered,
            time: chrono::Local::now(),
        }
    }

    // --- push_host ---

    #[test]
    fn push_host_single() {
        let state = state(None, None);
        state.push_host(test_host("host1"));
        let hosts = state.hosts.lock().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "host1");
    }

    #[test]
    fn push_host_eviction_at_max() {
        let state = state(None, None);
        for i in 0..TEST_MAX_HOSTS {
            state.push_host(test_host(&format!("host{i}")));
        }
        {
            let hosts = state.hosts.lock().unwrap();
            assert_eq!(hosts.len(), TEST_MAX_HOSTS);
        }
        // One more triggers eviction of 25%
        state.push_host(test_host("overflow"));
        let hosts = state.hosts.lock().unwrap();
        let evicted = TEST_MAX_HOSTS / 4;
        assert_eq!(hosts.len(), TEST_MAX_HOSTS - evicted + 1);
        assert!(hosts.iter().all(|h| h.hostname != "host0"));
        assert!(hosts.iter().any(|h| h.hostname == "overflow"));
    }

    // --- update_host_status ---

    #[test]
    fn update_host_status_found() {
        let state = state(None, None);
        state.push_host(test_host("myhost"));
        state.update_host_status("myhost", HostStatus::Installed);
        let hosts = state.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Installed));
    }

    #[test]
    fn update_host_status_updates_latest() {
        let state = state(None, None);
        state.push_host(test_host("myhost"));
        state.push_host(test_host("myhost"));
        state.update_host_status("myhost", HostStatus::SensorReady);
        let hosts = state.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Registered));
        assert!(matches!(hosts[1].status, HostStatus::SensorReady));
    }

    #[test]
    fn update_host_status_missing_noop() {
        let state = state(None, None);
        state.push_host(test_host("host1"));
        state.update_host_status("nonexistent", HostStatus::Installed);
        let hosts = state.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Registered));
    }

    // --- HostStatus::icon ---

    #[test]
    fn status_icons() {
        assert_eq!(HostStatus::Registered.icon(), "○");
        assert_eq!(HostStatus::SensorReady.icon(), "◐");
        assert_eq!(HostStatus::Installed.icon(), "●");
        assert_eq!(HostStatus::Failed("err".into()).icon(), "✕");
    }

    // --- base_url ---

    #[test]
    fn base_url_no_token_no_public() {
        assert_eq!(state(None, None).base_url(), "http://10.0.0.1:8080");
    }

    #[test]
    fn base_url_with_token() {
        assert_eq!(
            state(Some("abc123"), None).base_url(),
            "http://10.0.0.1:8080/abc123"
        );
    }

    #[test]
    fn base_url_with_public_url() {
        assert_eq!(
            state(None, Some("https://deploy.example.com")).base_url(),
            "https://deploy.example.com"
        );
    }

    #[test]
    fn base_url_public_url_with_token() {
        assert_eq!(
            state(Some("tok"), Some("https://deploy.example.com/")).base_url(),
            "https://deploy.example.com/tok"
        );
    }

    // --- base_url_with_host ---

    #[test]
    fn base_url_with_host_no_token() {
        assert_eq!(
            state(None, None).base_url_with_host("myhost:9090"),
            "http://myhost:9090"
        );
    }

    #[test]
    fn base_url_with_host_and_token() {
        assert_eq!(
            state(Some("t0k"), None).base_url_with_host("myhost:9090"),
            "http://myhost:9090/t0k"
        );
    }
}
