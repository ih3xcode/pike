use std::sync::{atomic::AtomicU32, Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::Notify;

use crate::common::net::detect_addr;
use crate::common::token::generate_token;
use crate::config::{self, ResolvedConfig, ServeArgs};
use crate::falcon::FalconClient;
use crate::sensors::loading::load_sensors;
use crate::sensors::{BinaryStore, MetadataCache, Sensor};
use crate::server::AppState;

use super::banner::print_banner;

pub fn run_serve(args: ServeArgs) -> Result<(), i32> {
    let cfg = resolve_config(&args)?;
    check_sensor_sources(&cfg)?;

    let sensors = load_local_sensors(&cfg)?;
    let token = pick_token(&cfg);
    let addr = advertised_addr(&cfg);

    let shutdown_notify = Arc::new(Notify::new());
    let rt = Runtime::new().expect("Failed to create tokio runtime");
    let update_handle = rt.spawn(crate::update::check_for_update());

    let (falcon, cid) = connect_api(&rt, &cfg)?;
    let has_api = falcon.is_some();
    let (metadata, store) = build_caches(&cfg, falcon)?;

    let sensor_lines: Vec<String> = sensors
        .iter()
        .map(|s| format!("  {} (SHA256: {})", s.filename, s.sha256))
        .collect();

    let state = Arc::new(AppState {
        token: token.clone(),
        cid,
        cloud: cfg.cloud.clone(),
        addr,
        port: cfg.port,
        public_url: cfg.public_url.clone(),
        local_sensors: sensors,
        metadata,
        store,
        download_count: AtomicU32::new(0),
        max_downloads: cfg.max_downloads,
        shutdown_notify: shutdown_notify.clone(),
        hosts: Mutex::new(Vec::new()),
        tags: crate::scripts::resolve_tags(cfg.tags.as_deref(), cfg.default_tag),
    });

    print_banner(&state, &cfg, token.as_deref(), &sensor_lines, has_api);

    if update_handle.is_finished() {
        if let Ok(Ok(Some(info))) = rt.block_on(update_handle) {
            eprintln!(
                "Update available: {} -> {} (run 'pike update --apply' to install)",
                info.current_version, info.latest_version
            );
        }
    }

    let bind_addr: std::net::SocketAddr = match format!("{}:{}", cfg.bind, cfg.port).parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ERROR: Invalid bind address: {e}");
            return Err(1);
        }
    };

    // A server error means a non-zero exit code. A busy port or a missing
    // CAP_NET_BIND_SERVICE would otherwise look like a successful start:
    // systemd would report `active` while pike kept restarting.
    rt.block_on(crate::server::run_server(
        state,
        bind_addr,
        cfg.timeout_minutes,
        shutdown_notify,
        true,
    ))
    .map_err(|e| {
        eprintln!("ERROR: {e}");
        1
    })
}

/// An explicit `--config` must exist; the default path is used only if present.
fn resolve_config(args: &ServeArgs) -> Result<ResolvedConfig, i32> {
    let file = match &args.config {
        Some(path) => config::load_file(path).map_err(|e| {
            eprintln!("ERROR: {e}");
            1
        })?,
        None => {
            let default_path = std::path::Path::new(config::defaults::DEFAULT_CONFIG_PATH);
            if default_path.exists() {
                let f = config::load_file(default_path).map_err(|e| {
                    eprintln!("ERROR: {e}");
                    1
                })?;
                eprintln!("[init] Using config {}", default_path.display());
                f
            } else {
                config::FileConfig::default()
            }
        }
    };

    config::resolve(args, file).map_err(|e| {
        eprintln!("ERROR: {e}");
        1
    })
}

fn check_sensor_sources(cfg: &ResolvedConfig) -> Result<(), i32> {
    let has_api_creds = cfg.client_id.is_some() && cfg.client_secret.is_some();
    if cfg.files.is_empty() && !has_api_creds {
        eprintln!("ERROR: provide --sensor or API credentials (--client-id/--client-secret).");
        return Err(1);
    }
    if cfg.cid.is_none() && !has_api_creds {
        eprintln!("ERROR: provide --cid or API credentials to fetch it.");
        return Err(1);
    }
    Ok(())
}

fn load_local_sensors(cfg: &ResolvedConfig) -> Result<Vec<Sensor>, i32> {
    if cfg.files.is_empty() {
        eprintln!("[init] No local sensor files provided");
        return Ok(Vec::new());
    }

    eprintln!("[init] Loading {} sensor file(s)...", cfg.files.len());
    let sensors = load_sensors(&cfg.files).map_err(|e| {
        eprintln!("ERROR: {e}");
        1
    })?;
    for sensor in &sensors {
        eprintln!(
            "[init]   {} ({} bytes, sha256={})",
            sensor.filename,
            sensor.data.len(),
            &sensor.sha256[..12]
        );
    }
    Ok(sensors)
}

fn pick_token(cfg: &ResolvedConfig) -> Option<String> {
    if !cfg.auth_enabled {
        eprintln!("[init] Token auth disabled");
        return None;
    }
    match cfg.token.clone() {
        Some(t) => Some(t),
        None => {
            let t = generate_token();
            eprintln!("[init] Generated token: {t}");
            eprintln!(
                "[init] WARNING: token is not pinned in config — the one-liner URL changes on every restart"
            );
            Some(t)
        }
    }
}

fn advertised_addr(cfg: &ResolvedConfig) -> String {
    cfg.addr.clone().unwrap_or_else(|| {
        eprintln!("[init] Auto-detecting local IP...");
        let a = detect_addr();
        eprintln!("[init] Detected address: {a}");
        a
    })
}

/// Authentication with a bounded number of attempts; `None` if the API never answered.
async fn authenticate_at_startup(
    client_id: &str,
    client_secret: &str,
    cloud: Option<&str>,
) -> Option<FalconClient> {
    const DELAYS_SECS: [u64; 2] = [5, 10];
    for attempt in 0..=DELAYS_SECS.len() {
        match FalconClient::new(client_id, client_secret, cloud).await {
            Ok(client) => return Some(client),
            Err(e) => {
                eprintln!("[falcon] Authentication attempt {} failed: {e}", attempt + 1);
                if let Some(delay) = DELAYS_SECS.get(attempt) {
                    eprintln!("[falcon] Retrying in {delay}s...");
                    tokio::time::sleep(Duration::from_secs(*delay)).await;
                }
            }
        }
    }
    None
}

/// The API client (when configured) and the CID the server will run with.
fn connect_api(rt: &Runtime, cfg: &ResolvedConfig) -> Result<(Option<Arc<FalconClient>>, String), i32> {
    let (Some(client_id), Some(client_secret)) = (&cfg.client_id, &cfg.client_secret) else {
        let cid = cfg.cid.clone().expect("checked by check_sensor_sources");
        eprintln!("[init] Local mode, CID: {cid}");
        return Ok((None, cid));
    };

    // Credentials are configured but authentication failed — exit non-zero.
    // Starting "half way" is not an option: without a client there is neither
    // fresh metadata nor access to the disk cache, so the server would serve
    // nothing but 404s. Under Restart=always systemd retries the process and
    // the failure stays visible in `systemctl status`.
    let Some(client) = rt.block_on(authenticate_at_startup(
        client_id,
        client_secret,
        cfg.cloud.as_deref(),
    )) else {
        eprintln!("ERROR: CrowdStrike API authentication failed; refusing to start.");
        eprintln!("HINT: check client_id/client_secret and the cloud region in the config.");
        return Err(1);
    };

    let cid = match cfg.cid.clone() {
        // An explicit CID — do not ask the API for one
        Some(cid) => cid,
        None => rt.block_on(client.get_ccid()).map_err(|e| {
            eprintln!("ERROR: cannot determine CID from API: {e}");
            eprintln!("HINT: set cid explicitly in the config.");
            1
        })?,
    };

    Ok((Some(Arc::new(client)), cid))
}

type Caches = (Option<Arc<MetadataCache>>, Option<Arc<BinaryStore>>);

fn build_caches(cfg: &ResolvedConfig, falcon: Option<Arc<FalconClient>>) -> Result<Caches, i32> {
    let Some(client) = falcon else {
        return Ok((None, None));
    };

    if let Err(e) = std::fs::create_dir_all(&cfg.cache_dir) {
        eprintln!(
            "ERROR: cannot create cache dir '{}': {e}",
            cfg.cache_dir.display()
        );
        return Err(1);
    }

    let metadata = MetadataCache::new(
        client.clone(),
        Duration::from_secs(cfg.metadata_ttl_minutes * 60),
    );
    let store = BinaryStore::new(client, cfg.cache_dir.clone(), cfg.cache_max_bytes);
    // No downloads of our own are in flight yet — anything in tmp/ is left
    // over from a previous run cut short mid-download
    store.sweep_tmp();

    Ok((Some(Arc::new(metadata)), Some(Arc::new(store))))
}
