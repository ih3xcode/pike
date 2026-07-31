use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/pike/pike.toml";

const DEFAULT_BIND: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_TIMEOUT_MINUTES: u64 = 0;
const DEFAULT_MAX_DOWNLOADS: u32 = 0;
const DEFAULT_METADATA_TTL_MINUTES: u64 = 60;
const DEFAULT_CACHE_MAX_BYTES: u64 = 21_474_836_480; // 20 ГіБ

/// Аргументи `pike serve`. Усі опційні — дефолти живуть у `resolve`,
/// інакше значення за замовчуванням від clap стало б невідрізнюваним
/// від явно переданого флага і завжди перебивало б конфіг.
#[derive(Debug, Default, clap::Args)]
pub struct ServeArgs {
    /// Шлях до конфіг-файлу (типово /etc/pike/pike.toml, якщо існує)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Адреса, на якій слухати
    #[arg(long)]
    pub bind: Option<String>,

    /// HTTP-порт
    #[arg(long)]
    pub port: Option<u16>,

    /// Адреса, яку показувати в ванлайнерах (автовизначення, якщо не задано)
    #[arg(long)]
    pub addr: Option<String>,

    /// Зовнішній URL (за reverse proxy), напр. https://pike.lab.local
    #[arg(long)]
    pub public_url: Option<String>,

    /// Токен автентифікації (генерується, якщо не задано)
    #[arg(long, env = "PIKE_TOKEN")]
    pub token: Option<String>,

    /// Таймаут у хвилинах, 0 = без обмеження
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Ліміт завантажень сенсорів, 0 = без обмеження
    #[arg(long)]
    pub max_downloads: Option<u32>,

    /// CrowdStrike API Client ID
    #[arg(long, env = "PIKE_CLIENT_ID")]
    pub client_id: Option<String>,

    /// CrowdStrike API Client Secret
    #[arg(long, env = "PIKE_CLIENT_SECRET")]
    pub client_secret: Option<String>,

    /// Хмара: us-1, us-2, eu-1, us-gov-1, us-gov-2
    #[arg(long)]
    pub cloud: Option<String>,

    /// CrowdStrike Customer ID
    #[arg(long, env = "PIKE_CID")]
    pub cid: Option<String>,

    /// Каталог кешу сенсорів
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Час життя списку сенсорів у хвилинах
    #[arg(long)]
    pub metadata_ttl: Option<u64>,

    /// Максимальний розмір кешу в байтах
    #[arg(long)]
    pub cache_max_bytes: Option<u64>,

    /// Теги групування, через кому
    #[arg(long)]
    pub tags: Option<String>,

    /// Не додавати типовий тег deployment/pike
    #[arg(long)]
    pub no_default_tag: bool,

    /// Локальний файл сенсора; можна вказати кілька разів
    #[arg(long = "sensor")]
    pub sensors: Vec<PathBuf>,

    /// Вимкнути автентифікацію за токеном
    #[arg(long)]
    pub no_auth: bool,
}

impl ServeArgs {
    /// Розбір лише для тестів — дає доступ до логіки clap разом із env.
    #[cfg(test)]
    pub fn parse_from_args(argv: &[&str]) -> Self {
        // `derive(Args)` не дає CommandFactory — команду будуємо самі
        use clap::{Args, FromArgMatches};
        let cmd = <Self as Args>::augment_args(clap::Command::new("pike"));
        let matches = cmd.get_matches_from(argv);
        Self::from_arg_matches(&matches).expect("valid args")
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub falcon: FalconSection,
    #[serde(default)]
    pub sensors: SensorsSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSection {
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub addr: Option<String>,
    pub public_url: Option<String>,
    pub token: Option<String>,
    pub timeout_minutes: Option<u64>,
    pub max_downloads: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FalconSection {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cloud: Option<String>,
    pub cid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorsSection {
    pub cache_dir: Option<PathBuf>,
    pub metadata_ttl_minutes: Option<u64>,
    pub cache_max_bytes: Option<u64>,
    pub tags: Option<String>,
    pub default_tag: Option<bool>,
    pub files: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub bind: String,
    pub port: u16,
    pub addr: Option<String>,
    pub public_url: Option<String>,
    pub auth_enabled: bool,
    pub token: Option<String>,
    pub timeout_minutes: u64,
    pub max_downloads: u32,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cloud: Option<String>,
    pub cid: Option<String>,
    pub cache_dir: PathBuf,
    pub metadata_ttl_minutes: u64,
    pub cache_max_bytes: u64,
    pub tags: Option<String>,
    pub default_tag: bool,
    pub files: Vec<PathBuf>,
}

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

pub fn load_file(path: &Path) -> Result<FileConfig, AppError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AppError::io(format!("Cannot read config '{}'", path.display()), e))?;
    toml::from_str(&text)
        .map_err(|e| AppError::Other(format!("Invalid config '{}': {e}", path.display())))
}

/// Порожній рядок у TOML означає «не задано».
fn blank_to_none(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Токен потрапляє в шлях URL — дозволені лише безпечні там символи.
fn validate_token(token: &str) -> Result<(), AppError> {
    let ok = !token.is_empty()
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Invalid token '{token}': use 1-128 chars from [A-Za-z0-9_-]"
        )))
    }
}

pub fn resolve(args: &ServeArgs, file: FileConfig) -> Result<ResolvedConfig, AppError> {
    let token = if args.no_auth {
        None
    } else {
        let t = args
            .token
            .clone()
            .or_else(|| blank_to_none(file.server.token.clone()));
        if let Some(ref t) = t {
            validate_token(t)?;
        }
        t
    };

    let mut files = args.sensors.clone();
    if files.is_empty() {
        files = file.sensors.files.clone().unwrap_or_default();
    }

    Ok(ResolvedConfig {
        bind: args
            .bind
            .clone()
            .or_else(|| blank_to_none(file.server.bind))
            .unwrap_or_else(|| DEFAULT_BIND.to_string()),
        port: args.port.or(file.server.port).unwrap_or(DEFAULT_PORT),
        addr: args.addr.clone().or_else(|| blank_to_none(file.server.addr)),
        public_url: args
            .public_url
            .clone()
            .or_else(|| blank_to_none(file.server.public_url))
            .map(|u| u.trim_end_matches('/').to_string()),
        auth_enabled: !args.no_auth,
        token,
        timeout_minutes: args
            .timeout
            .or(file.server.timeout_minutes)
            .unwrap_or(DEFAULT_TIMEOUT_MINUTES),
        max_downloads: args
            .max_downloads
            .or(file.server.max_downloads)
            .unwrap_or(DEFAULT_MAX_DOWNLOADS),
        client_id: args
            .client_id
            .clone()
            .or_else(|| blank_to_none(file.falcon.client_id)),
        client_secret: args
            .client_secret
            .clone()
            .or_else(|| blank_to_none(file.falcon.client_secret)),
        cloud: args
            .cloud
            .clone()
            .or_else(|| blank_to_none(file.falcon.cloud)),
        cid: args.cid.clone().or_else(|| blank_to_none(file.falcon.cid)),
        cache_dir: args
            .cache_dir
            .clone()
            .or(file.sensors.cache_dir)
            .unwrap_or_else(default_cache_dir),
        metadata_ttl_minutes: args
            .metadata_ttl
            .or(file.sensors.metadata_ttl_minutes)
            .unwrap_or(DEFAULT_METADATA_TTL_MINUTES),
        cache_max_bytes: args
            .cache_max_bytes
            .or(file.sensors.cache_max_bytes)
            .unwrap_or(DEFAULT_CACHE_MAX_BYTES),
        tags: args
            .tags
            .clone()
            .or_else(|| blank_to_none(file.sensors.tags)),
        default_tag: if args.no_default_tag {
            false
        } else {
            file.sensors.default_tag.unwrap_or(true)
        },
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Змінні середовища глобальні для процесу — тести, що їх чіпають,
    // мусять іти по черзі.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn empty_args() -> ServeArgs {
        ServeArgs::default()
    }

    fn file_with_port(port: u16) -> FileConfig {
        let mut f = FileConfig::default();
        f.server.port = Some(port);
        f
    }

    // --- дефолти ---

    #[test]
    fn defaults_when_nothing_set() {
        let cfg = resolve(&empty_args(), FileConfig::default()).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.timeout_minutes, 0);
        assert_eq!(cfg.max_downloads, 0);
        assert_eq!(cfg.metadata_ttl_minutes, 60);
        assert_eq!(cfg.cache_max_bytes, 21_474_836_480);
        assert!(cfg.default_tag);
        assert!(cfg.auth_enabled);
        assert!(cfg.token.is_none());
    }

    // --- пріоритет ---

    #[test]
    fn file_value_used_when_no_arg() {
        let cfg = resolve(&empty_args(), file_with_port(9090)).unwrap();
        assert_eq!(cfg.port, 9090);
    }

    #[test]
    fn arg_overrides_file() {
        let mut args = empty_args();
        args.port = Some(7070);
        let cfg = resolve(&args, file_with_port(9090)).unwrap();
        assert_eq!(cfg.port, 7070);
    }

    #[test]
    fn env_overrides_file_and_arg_overrides_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("PIKE_CLIENT_ID", "from-env") };

        // clap читає env під час розбору
        let args = ServeArgs::parse_from_args(&["pike"]);
        let cfg = resolve(&args, FileConfig::default()).unwrap();
        assert_eq!(cfg.client_id.as_deref(), Some("from-env"));

        let args = ServeArgs::parse_from_args(&["pike", "--client-id", "from-flag"]);
        let cfg = resolve(&args, FileConfig::default()).unwrap();
        assert_eq!(cfg.client_id.as_deref(), Some("from-flag"));

        unsafe { std::env::remove_var("PIKE_CLIENT_ID") };
    }

    // --- порожні рядки в TOML ---

    #[test]
    fn blank_strings_in_file_are_unset() {
        let file: FileConfig = toml::from_str(
            r#"
            [server]
            public_url = ""
            [falcon]
            cid = ""
            "#,
        )
        .unwrap();
        let cfg = resolve(&empty_args(), file).unwrap();
        assert!(cfg.public_url.is_none());
        assert!(cfg.cid.is_none());
    }

    // --- токен ---

    #[test]
    fn token_from_file_is_kept() {
        let mut file = FileConfig::default();
        file.server.token = Some("abc123DEF".into());
        let cfg = resolve(&empty_args(), file).unwrap();
        assert_eq!(cfg.token.as_deref(), Some("abc123DEF"));
    }

    #[test]
    fn token_with_url_unsafe_chars_rejected() {
        let mut file = FileConfig::default();
        file.server.token = Some("bad/token".into());
        assert!(resolve(&empty_args(), file).is_err());
    }

    #[test]
    fn no_auth_clears_token() {
        let mut args = empty_args();
        args.no_auth = true;
        let mut file = FileConfig::default();
        file.server.token = Some("abc123".into());
        let cfg = resolve(&args, file).unwrap();
        assert!(!cfg.auth_enabled);
        assert!(cfg.token.is_none());
    }

    // --- теги ---

    #[test]
    fn no_default_tag_flag_overrides_file() {
        let mut args = empty_args();
        args.no_default_tag = true;
        let mut file = FileConfig::default();
        file.sensors.default_tag = Some(true);
        let cfg = resolve(&args, file).unwrap();
        assert!(!cfg.default_tag);
    }

    // --- розбір файлу ---

    #[test]
    fn unknown_key_is_an_error() {
        let err = toml::from_str::<FileConfig>("[server]\nnonsense = 1\n");
        assert!(err.is_err());
    }
}
