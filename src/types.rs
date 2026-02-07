use std::sync::{atomic::AtomicU32, Arc, Mutex};
use tokio::sync::{Notify, RwLock};

use crate::falcon_api::FalconClient;

const MAX_HOSTS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    Deb,
    Rpm,
    WindowsExe,
}

pub struct Sensor {
    pub filename: String,
    pub data: bytes::Bytes,
    pub sha256: String,
    pub sensor_type: SensorType,
}

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

pub struct AppState {
    pub token: Option<String>,
    pub cid: String,
    pub cloud: Option<String>,
    pub addr: String,
    pub port: u16,
    pub public_url: Option<String>,
    pub sensors: RwLock<Vec<Sensor>>,
    pub download_count: AtomicU32,
    pub max_downloads: u32,
    pub shutdown_notify: Arc<Notify>,
    pub falcon_client: Option<FalconClient>,
    pub hosts: Mutex<Vec<HostEntry>>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app_state() -> Arc<AppState> {
        Arc::new(AppState {
            token: None,
            cid: String::new(),
            cloud: None,
            addr: "127.0.0.1".into(),
            port: 8080,
            public_url: None,
            sensors: RwLock::new(vec![]),
            download_count: AtomicU32::new(0),
            max_downloads: 0,
            shutdown_notify: Arc::new(Notify::new()),
            falcon_client: None,
            hosts: Mutex::new(vec![]),
        })
    }

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
        let state = test_app_state();
        state.push_host(test_host("host1"));
        let hosts = state.hosts.lock().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "host1");
    }

    #[test]
    fn push_host_eviction_at_max() {
        let state = test_app_state();
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
        let state = test_app_state();
        state.push_host(test_host("myhost"));
        state.update_host_status("myhost", HostStatus::Installed);
        let hosts = state.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Installed));
    }

    #[test]
    fn update_host_status_updates_latest() {
        let state = test_app_state();
        state.push_host(test_host("myhost"));
        state.push_host(test_host("myhost"));
        state.update_host_status("myhost", HostStatus::SensorReady);
        let hosts = state.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Registered));
        assert!(matches!(hosts[1].status, HostStatus::SensorReady));
    }

    #[test]
    fn update_host_status_missing_noop() {
        let state = test_app_state();
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
}
