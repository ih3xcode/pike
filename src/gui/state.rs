use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::common::net::detect_available_addrs;

pub const CLOUD_OPTIONS: &[&str] = &["us-1", "us-2", "eu-1", "us-gov-1", "us-gov-2"];

/// Усе, що редагується на екрані конфігурації. Живе рівно стільки, скільки
/// показаний екран; те, що має пережити перезапуск сервера, копіюється
/// в [`super::persist::SavedConfig`].
pub(super) struct ConfigState {
    pub config_tab: usize,

    // API tab
    pub client_id: String,
    pub client_secret: String,
    pub cloud_idx: usize,

    // Files tab
    pub sensor_paths: Vec<PathBuf>,
    pub cid: String,
    pub pending_files: Arc<Mutex<Vec<PathBuf>>>,
    pub file_dialog_open: bool,

    // Network
    pub available_addrs: Vec<(String, String)>,
    pub selected_addr_idx: usize,
    pub port: String,
    pub custom_url_enabled: bool,
    pub custom_url: String,

    // Sensor
    pub tags: String,

    // Advanced
    pub show_advanced: bool,
    pub timeout: String,
    pub max_downloads: String,
    pub auth_enabled: bool,
    pub no_default_tag: bool,

    pub error: Option<String>,
    pub start_requested: bool,
    pub update_requested: bool,
}

pub(super) fn new_config_state() -> ConfigState {
    ConfigState {
        config_tab: 0,
        client_id: String::new(),
        client_secret: String::new(),
        cloud_idx: 2, // eu-1
        sensor_paths: Vec::new(),
        cid: String::new(),
        pending_files: Arc::new(Mutex::new(Vec::new())),
        file_dialog_open: false,
        available_addrs: detect_available_addrs(),
        selected_addr_idx: 0,
        port: "8080".into(),
        custom_url_enabled: false,
        custom_url: String::new(),
        tags: String::new(),
        show_advanced: false,
        timeout: "30".into(),
        max_downloads: "0".into(),
        auth_enabled: true,
        no_default_tag: false,
        error: None,
        start_requested: false,
        update_requested: false,
    }
}
