use std::path::PathBuf;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/pike/pike.toml";

pub const DEFAULT_BIND: &str = "0.0.0.0";
pub const DEFAULT_PORT: u16 = 8080;
pub const DEFAULT_TIMEOUT_MINUTES: u64 = 0;
pub const DEFAULT_MAX_DOWNLOADS: u32 = 0;
/// Регіон за замовчуванням. Без нього `api_base_url` мовчки бере us-1,
/// і EU-тенант отримує 401 на старті або реєструє хости не в ту хмару.
pub const DEFAULT_CLOUD: &str = "eu-1";
pub const DEFAULT_METADATA_TTL_MINUTES: u64 = 60;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 21_474_836_480; // 20 ГіБ
/// Каталог кешу для установки під systemd.
pub const DEFAULT_SERVICE_CACHE_DIR: &str = "/var/cache/pike";

/// Каталог кешу за замовчуванням для запуску не під systemd.
/// Сервісний конфіг завжди задає шлях явно.
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
