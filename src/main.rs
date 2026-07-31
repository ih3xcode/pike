#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod error;
mod falcon_api;
mod gui;
mod scripts;
mod sensor_match;
mod server;
mod shutdown;
mod types;
pub mod update;
mod util;

use clap::{Parser, Subcommand};
use std::sync::{atomic::AtomicU32, Arc, Mutex};
use tokio::sync::{Notify, RwLock};

use types::AppState;
use util::{detect_addr, generate_token, load_sensors};

#[derive(Parser)]
#[command(name = "pike", about = "CrowdStrike sensor deployment tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Запустити GUI (типово, якщо команду не вказано)
    Gui,
    /// Запустити HTTP-сервер розгортання
    Serve(config::ServeArgs),
    /// Перевірити наявність оновлень і за потреби встановити
    Update {
        #[arg(long)]
        apply: bool,
    },
}

fn main() {
    // Re-attach to parent console so CLI output works even with windows_subsystem = "windows".
    // Succeeds when launched from a terminal; silently fails on double-click (no parent console).
    #[cfg(target_os = "windows")]
    {
        unsafe extern "system" {
            fn AttachConsole(id: u32) -> i32;
        }
        unsafe { AttachConsole(0xFFFFFFFF); } // ATTACH_PARENT_PROCESS
    }

    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Gui) => gui::run_gui(),
        Some(Command::Update { apply }) => {
            if let Err(code) = run_update_command(apply) {
                std::process::exit(code);
            }
        }
        Some(Command::Serve(args)) => {
            if let Err(code) = run_serve(args) {
                std::process::exit(code);
            }
        }
    }
}

fn run_update_command(apply: bool) -> Result<(), i32> {
    eprintln!(
        "pike {} — checking for updates...",
        update::CURRENT_VERSION
    );

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let info = match rt.block_on(update::check_for_update()) {
        Ok(Some(info)) => info,
        Ok(None) => {
            eprintln!("Already up to date.");
            return Ok(());
        }
        Err(e) => {
            eprintln!("ERROR: Failed to check for updates: {e}");
            return Err(1);
        }
    };

    eprintln!(
        "Update available: {} -> {}",
        info.current_version, info.latest_version
    );
    eprintln!("Release: {}", info.release_url);

    if !apply {
        eprintln!("Run 'pike update --apply' to install the update.");
        return Ok(());
    }

    eprintln!(
        "Downloading pike {} ({} bytes)...",
        info.latest_version, info.asset_size
    );

    match rt.block_on(update::apply_update(&info)) {
        Ok(()) => {
            eprintln!(
                "Successfully updated to pike {}. Restart pike to use the new version.",
                info.latest_version
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: Failed to apply update: {e}");
            Err(1)
        }
    }
}

fn print_banner(
    state: &AppState,
    cfg: &config::ResolvedConfig,
    token: Option<&str>,
    sensor_lines: &[String],
    has_api: bool,
) {
    let base_url = state.base_url();
    let max_dl = if cfg.max_downloads == 0 {
        "unlimited".to_string()
    } else {
        cfg.max_downloads.to_string()
    };
    let timeout = if cfg.timeout_minutes == 0 {
        "none".to_string()
    } else {
        format!("{} min", cfg.timeout_minutes)
    };

    eprintln!("pike — CrowdStrike sensor deployer");
    eprintln!("─────────────────────────────────────────");
    eprintln!("Server:    http://{}:{}", state.addr, cfg.port);
    match token {
        Some(t) => eprintln!("Token:     {t}"),
        None => eprintln!("Auth:      disabled"),
    }
    if let Some(url) = &cfg.public_url {
        eprintln!("Public:    {url}");
    }
    if has_api {
        eprintln!("API:       connected");
    }
    if let Some(tags) = &state.tags {
        eprintln!("Tags:      {tags}");
    }
    eprintln!("Cache:     {}", cfg.cache_dir.display());
    eprintln!("Timeout:   {timeout} | Max downloads: {max_dl}");

    if !sensor_lines.is_empty() {
        eprintln!("Sensors:");
        for line in sensor_lines {
            eprintln!("{line}");
        }
    } else if has_api {
        eprintln!("Sensors:   on-demand via API");
    }

    eprintln!();
    eprintln!("Linux:");
    eprintln!("  curl -fsS {base_url}/lin | sudo bash");
    eprintln!();
    eprintln!("Windows (Run as Administrator):");
    eprintln!("  irm {base_url}/win | iex");
    eprintln!();
}

fn run_serve(args: config::ServeArgs) -> Result<(), i32> {
    // Конфіг: явний --config обовʼязково має існувати; типовий шлях — лише якщо є
    let file = match &args.config {
        Some(path) => match config::load_file(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(1);
            }
        },
        None => {
            let default_path = std::path::Path::new(config::DEFAULT_CONFIG_PATH);
            if default_path.exists() {
                match config::load_file(default_path) {
                    Ok(f) => {
                        eprintln!("[init] Using config {}", default_path.display());
                        f
                    }
                    Err(e) => {
                        eprintln!("ERROR: {e}");
                        return Err(1);
                    }
                }
            } else {
                config::FileConfig::default()
            }
        }
    };

    let cfg = match config::resolve(&args, file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return Err(1);
        }
    };

    let has_api_creds = cfg.client_id.is_some() && cfg.client_secret.is_some();
    if cfg.files.is_empty() && !has_api_creds {
        eprintln!("ERROR: provide --sensor or API credentials (--client-id/--client-secret).");
        return Err(1);
    }
    if cfg.cid.is_none() && !has_api_creds {
        eprintln!("ERROR: provide --cid or API credentials to fetch it.");
        return Err(1);
    }

    let sensors = if cfg.files.is_empty() {
        eprintln!("[init] No local sensor files provided");
        Vec::new()
    } else {
        eprintln!("[init] Loading {} sensor file(s)...", cfg.files.len());
        match load_sensors(&cfg.files) {
            Ok(s) => {
                for sensor in &s {
                    eprintln!(
                        "[init]   {} ({} bytes, sha256={})",
                        sensor.filename,
                        sensor.data.len(),
                        &sensor.sha256[..12]
                    );
                }
                s
            }
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(1);
            }
        }
    };

    let token = if !cfg.auth_enabled {
        eprintln!("[init] Token auth disabled");
        None
    } else {
        match cfg.token.clone() {
            Some(t) => Some(t),
            None => {
                let t = generate_token();
                eprintln!("[init] Generated token: {t}");
                eprintln!("[init] WARNING: token is not pinned in config — the one-liner URL changes on every restart");
                Some(t)
            }
        }
    };

    let addr = cfg.addr.clone().unwrap_or_else(|| {
        eprintln!("[init] Auto-detecting local IP...");
        let a = detect_addr();
        eprintln!("[init] Detected address: {a}");
        a
    });

    let shutdown_notify = Arc::new(Notify::new());
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let update_handle = rt.spawn(update::check_for_update());

    let (falcon_client, cid) = if has_api_creds {
        let client_id = cfg.client_id.clone().unwrap();
        let client_secret = cfg.client_secret.clone().unwrap();
        let cloud = cfg.cloud.clone();
        let explicit_cid = cfg.cid.clone();

        let result: Result<_, error::AppError> = rt.block_on(async move {
            let client =
                falcon_api::FalconClient::new(&client_id, &client_secret, cloud.as_deref()).await?;
            let api_cid = if explicit_cid.is_none() {
                Some(client.get_ccid().await?)
            } else {
                None
            };
            Ok((client, api_cid))
        });

        match result {
            Ok((client, api_cid)) => {
                let cid = cfg.cid.clone().unwrap_or_else(|| api_cid.unwrap());
                (Some(client), cid)
            }
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(1);
            }
        }
    } else {
        let cid = cfg.cid.clone().unwrap();
        eprintln!("[init] Local mode, CID: {cid}");
        (None, cid)
    };

    let sensor_lines: Vec<String> = sensors
        .iter()
        .map(|s| format!("  {} (SHA256: {})", s.filename, s.sha256))
        .collect();
    let has_api = falcon_client.is_some();

    let state = Arc::new(AppState {
        token: token.clone(),
        cid,
        cloud: cfg.cloud.clone(),
        addr: addr.clone(),
        port: cfg.port,
        public_url: cfg.public_url.clone(),
        sensors: RwLock::new(sensors),
        download_count: AtomicU32::new(0),
        max_downloads: cfg.max_downloads,
        shutdown_notify: shutdown_notify.clone(),
        falcon_client,
        hosts: Mutex::new(Vec::new()),
        tags: scripts::resolve_tags(cfg.tags.as_deref(), cfg.default_tag),
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

    rt.block_on(async {
        if let Err(e) = server::run_server(
            state,
            bind_addr,
            cfg.timeout_minutes,
            shutdown_notify,
            true,
        )
        .await
        {
            eprintln!("ERROR: {e}");
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_args_means_gui() {
        let cli = Cli::parse_from(["pike"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn gui_subcommand_parses() {
        let cli = Cli::parse_from(["pike", "gui"]);
        assert!(matches!(cli.command, Some(Command::Gui)));
    }

    #[test]
    fn serve_subcommand_parses_flags() {
        let cli = Cli::parse_from(["pike", "serve", "--port", "9090", "--no-auth"]);
        let Some(Command::Serve(args)) = cli.command else {
            panic!("expected serve");
        };
        assert_eq!(args.port, Some(9090));
        assert!(args.no_auth);
    }

    #[test]
    fn old_top_level_flags_are_rejected() {
        // Стара форма `pike --sensor x --cid y` більше не підтримується
        assert!(Cli::try_parse_from(["pike", "--sensor", "x.deb"]).is_err());
    }

    #[test]
    fn update_subcommand_still_parses() {
        let cli = Cli::parse_from(["pike", "update", "--apply"]);
        assert!(matches!(cli.command, Some(Command::Update { apply: true })));
    }
}
