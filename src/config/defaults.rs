use std::path::PathBuf;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/pike/pike.toml";

pub const DEFAULT_BIND: &str = "0.0.0.0";
pub const DEFAULT_PORT: u16 = 8080;
pub const DEFAULT_TIMEOUT_MINUTES: u64 = 0;
pub const DEFAULT_MAX_DOWNLOADS: u32 = 0;
/// Default region. Without it `api_base_url` silently falls back to us-1,
/// so an EU tenant gets a 401 at startup or registers hosts in the wrong cloud.
pub const DEFAULT_CLOUD: &str = "eu-1";
pub const DEFAULT_METADATA_TTL_MINUTES: u64 = 60;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 21_474_836_480; // 20 GiB
/// Cache directory for a systemd installation.
pub const DEFAULT_SERVICE_CACHE_DIR: &str = "/var/cache/pike";

/// Default cache directory when not running under systemd.
/// The service config always sets the path explicitly.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("pike");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".cache").join("pike");
        }
    }
    std::env::temp_dir().join("pike-cache")
}
