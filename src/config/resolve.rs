use std::path::PathBuf;

use crate::common::AppError;

use super::args::ServeArgs;
use super::defaults::*;
use super::file::FileConfig;
use super::validate::{validate_cloud, validate_token};

/// The config after all sources are merged. Nothing downstream reads
/// anything else — no module looks at the arguments or the file directly.
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

/// An empty string means "not set", wherever it came from. Flags and env
/// vars need this as much as the file does: clap hands back an env var's
/// value verbatim, so `PIKE_CID=` in an EnvironmentFile used to produce a
/// server that started happily and then 404-ed every install-script request.
fn blank_to_none(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn resolve(args: &ServeArgs, file: FileConfig) -> Result<ResolvedConfig, AppError> {
    let token = if args.no_auth {
        None
    } else {
        let t = blank_to_none(args.token.clone())
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

    let cloud = blank_to_none(args.cloud.clone())
        .or_else(|| blank_to_none(file.falcon.cloud))
        .unwrap_or_else(|| DEFAULT_CLOUD.to_string());
    validate_cloud(&cloud)?;

    Ok(ResolvedConfig {
        bind: blank_to_none(args.bind.clone())
            .or_else(|| blank_to_none(file.server.bind))
            .unwrap_or_else(|| DEFAULT_BIND.to_string()),
        port: args.port.or(file.server.port).unwrap_or(DEFAULT_PORT),
        addr: blank_to_none(args.addr.clone()).or_else(|| blank_to_none(file.server.addr)),
        public_url: blank_to_none(args.public_url.clone())
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
        client_id: blank_to_none(args.client_id.clone())
            .or_else(|| blank_to_none(file.falcon.client_id)),
        client_secret: blank_to_none(args.client_secret.clone())
            .or_else(|| blank_to_none(file.falcon.client_secret)),
        cloud: Some(cloud),
        cid: blank_to_none(args.cid.clone()).or_else(|| blank_to_none(file.falcon.cid)),
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
        tags: blank_to_none(args.tags.clone()).or_else(|| blank_to_none(file.sensors.tags)),
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

    // Environment variables are process-global — tests that touch them must
    // run one at a time.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn empty_args() -> ServeArgs {
        ServeArgs::default()
    }

    fn file_with_port(port: u16) -> FileConfig {
        let mut f = FileConfig::default();
        f.server.port = Some(port);
        f
    }

    // --- defaults ---

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
        // Without the default, api_base_url would silently pick us-1
        assert_eq!(cfg.cloud.as_deref(), Some("eu-1"));
    }

    #[test]
    fn cloud_from_file_beats_default() {
        let mut file = FileConfig::default();
        file.falcon.cloud = Some("us-2".into());
        let cfg = resolve(&empty_args(), file).unwrap();
        assert_eq!(cfg.cloud.as_deref(), Some("us-2"));
    }

    #[test]
    fn cloud_arg_beats_file() {
        let mut args = empty_args();
        args.cloud = Some("us-gov-1".into());
        let mut file = FileConfig::default();
        file.falcon.cloud = Some("us-2".into());
        let cfg = resolve(&args, file).unwrap();
        assert_eq!(cfg.cloud.as_deref(), Some("us-gov-1"));
    }

    // --- precedence ---

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

        // clap reads env vars while parsing
        let args = ServeArgs::parse_from_args(&["pike"]);
        let cfg = resolve(&args, FileConfig::default()).unwrap();
        assert_eq!(cfg.client_id.as_deref(), Some("from-env"));

        let args = ServeArgs::parse_from_args(&["pike", "--client-id", "from-flag"]);
        let cfg = resolve(&args, FileConfig::default()).unwrap();
        assert_eq!(cfg.client_id.as_deref(), Some("from-flag"));

        unsafe { std::env::remove_var("PIKE_CLIENT_ID") };
    }

    // --- empty strings in TOML ---

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

    // --- token ---

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

    // --- blank flag and env values ---

    #[test]
    fn blank_env_values_are_treated_as_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("PIKE_CID", "") };
        unsafe { std::env::set_var("PIKE_TOKEN", "") };

        let args = ServeArgs::parse_from_args(&["pike"]);
        let cfg = resolve(&args, FileConfig::default()).unwrap();

        // Previously `Some("")`: the server started and then 404-ed /lin
        assert!(cfg.cid.is_none());
        // and an empty token failed validation instead of being ignored
        assert!(cfg.token.is_none());

        unsafe { std::env::remove_var("PIKE_CID") };
        unsafe { std::env::remove_var("PIKE_TOKEN") };
    }

    #[test]
    fn blank_flag_value_falls_through_to_the_file() {
        let mut args = empty_args();
        args.cid = Some("   ".into());
        let mut file = FileConfig::default();
        file.falcon.cid = Some("FROM-FILE".into());
        let cfg = resolve(&args, file).unwrap();
        assert_eq!(cfg.cid.as_deref(), Some("FROM-FILE"));
    }

    // --- cloud ---

    #[test]
    fn unknown_cloud_is_an_error() {
        let mut args = empty_args();
        args.cloud = Some("eu1".into());
        assert!(resolve(&args, FileConfig::default()).is_err());
    }

    #[test]
    fn unknown_cloud_in_the_file_is_an_error() {
        let mut file = FileConfig::default();
        file.falcon.cloud = Some("europe".into());
        assert!(resolve(&empty_args(), file).is_err());
    }

    // --- tags ---

    #[test]
    fn no_default_tag_flag_overrides_file() {
        let mut args = empty_args();
        args.no_default_tag = true;
        let mut file = FileConfig::default();
        file.sensors.default_tag = Some(true);
        let cfg = resolve(&args, file).unwrap();
        assert!(!cfg.default_tag);
    }
}
