use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::common::net::detect_available_addrs;

use super::state::ConfigState;

/// Знімок відповідей, який переживає запуск і зупинку сервера.
/// Навмисно без ефемерного: помилок, прапорців діалогу й списку адрес —
/// адреси перевизначаються на кожному поверненні на екран.
#[derive(Clone)]
pub(super) struct SavedConfig {
    pub config_tab: usize,
    pub client_id: String,
    pub client_secret: String,
    pub cloud_idx: usize,
    pub sensor_paths: Vec<PathBuf>,
    pub cid: String,
    pub selected_addr_idx: usize,
    pub port: String,
    pub custom_url_enabled: bool,
    pub custom_url: String,
    pub tags: String,
    pub timeout: String,
    pub max_downloads: String,
    pub auth_enabled: bool,
    pub no_default_tag: bool,
}

pub(super) fn make_saved_config(config: &ConfigState) -> SavedConfig {
    SavedConfig {
        config_tab: config.config_tab,
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        cloud_idx: config.cloud_idx,
        sensor_paths: config.sensor_paths.clone(),
        cid: config.cid.clone(),
        selected_addr_idx: config.selected_addr_idx,
        port: config.port.clone(),
        custom_url_enabled: config.custom_url_enabled,
        custom_url: config.custom_url.clone(),
        tags: config.tags.clone(),
        timeout: config.timeout.clone(),
        max_downloads: config.max_downloads.clone(),
        auth_enabled: config.auth_enabled,
        no_default_tag: config.no_default_tag,
    }
}

pub(super) fn config_from_saved(saved: SavedConfig) -> ConfigState {
    let available_addrs = detect_available_addrs();
    // Інтерфейс міг зникнути, поки сервер працював — індекс поза списком
    // означав би паніку при відмальовуванні комбобокса
    let selected_addr_idx = if saved.selected_addr_idx < available_addrs.len() {
        saved.selected_addr_idx
    } else {
        0
    };
    ConfigState {
        config_tab: saved.config_tab,
        client_id: saved.client_id,
        client_secret: saved.client_secret,
        cloud_idx: saved.cloud_idx,
        sensor_paths: saved.sensor_paths,
        cid: saved.cid,
        pending_files: Arc::new(Mutex::new(Vec::new())),
        file_dialog_open: false,
        available_addrs,
        selected_addr_idx,
        port: saved.port,
        custom_url_enabled: saved.custom_url_enabled,
        custom_url: saved.custom_url,
        tags: saved.tags,
        show_advanced: false,
        timeout: saved.timeout,
        max_downloads: saved.max_downloads,
        auth_enabled: saved.auth_enabled,
        no_default_tag: saved.no_default_tag,
        error: None,
        start_requested: false,
        update_requested: false,
    }
}
