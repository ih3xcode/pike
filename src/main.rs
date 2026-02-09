#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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

    /// Sensor installer file(s) — type auto-detected from extension (.deb, .rpm, .exe)
    #[arg(long = "sensor")]
    sensors: Vec<std::path::PathBuf>,

    /// CrowdStrike Customer ID
    #[arg(long)]
    cid: Option<String>,

    /// CrowdStrike cloud (us-1, us-2, eu-1, us-gov-1, us-gov-2)
    #[arg(long, default_value = "eu-1")]
    cloud: Option<String>,

    /// CrowdStrike API Client ID
    #[arg(long)]
    client_id: Option<String>,

    /// CrowdStrike API Client Secret
    #[arg(long)]
    client_secret: Option<String>,

    /// Advertised address for one-liners (auto-detect if omitted)
    #[arg(long)]
    addr: Option<String>,

    /// HTTP port
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Listen/bind address
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Timeout in minutes
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Max sensor binary downloads (0 = unlimited)
    #[arg(long, default_value = "0")]
    max_downloads: u32,

    /// Force GUI mode
    #[arg(long)]
    gui: bool,

    /// Disable token authentication
    #[arg(long)]
    no_auth: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Check for updates and optionally install them
    Update {
        /// Download and install the update
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

    // Handle subcommands before GUI/CLI logic
    if let Some(command) = &cli.command {
        match command {
            Command::Update { apply } => {
                if let Err(code) = run_update_command(*apply) {
                    std::process::exit(code);
                }
                return;
            }
        }
    }

    // GUI mode: no sensors and no API credentials provided, or --gui flag
    let has_api_creds = cli.client_id.is_some() && cli.client_secret.is_some();
    let is_gui = cli.gui || (cli.sensors.is_empty() && cli.cid.is_none() && !has_api_creds);

    if is_gui {
        gui::run_gui();
        return;
    }

    if let Err(code) = run_cli(cli, has_api_creds) {
        std::process::exit(code);
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

fn run_cli(cli: Cli, has_api_creds: bool) -> Result<(), i32> {
    // CLI mode — require either sensors+CID or API credentials
    if cli.sensors.is_empty() && !has_api_creds {
        eprintln!("ERROR: --sensor or --client-id/--client-secret is required in CLI mode.");
        return Err(1);
    }

    if cli.cid.is_none() && !has_api_creds {
        eprintln!("ERROR: --cid or --client-id/--client-secret is required in CLI mode.");
        return Err(1);
    }

    let sensors = if cli.sensors.is_empty() {
        eprintln!("[init] No local sensor files provided");
        Vec::new()
    } else {
        eprintln!("[init] Loading {} sensor file(s)...", cli.sensors.len());
        match load_sensors(&cli.sensors) {
            Ok(s) => {
                for sensor in &s {
                    eprintln!("[init]   {} ({} bytes, sha256={})", sensor.filename, sensor.data.len(), &sensor.sha256[..12]);
                }
                s
            }
            Err(e) => {
                eprintln!("ERROR: {}", e);
                return Err(1);
            }
        }
    };

    let token = if cli.no_auth {
        eprintln!("[init] Token auth disabled");
        None
    } else {
        let t = generate_token();
        eprintln!("[init] Generated token: {t}");
        Some(t)
    };
    let addr = cli.addr.unwrap_or_else(|| {
        eprintln!("[init] Auto-detecting local IP...");
        let a = detect_addr();
        eprintln!("[init] Detected address: {a}");
        a
    });
    let shutdown_notify = Arc::new(Notify::new());

    // Build runtime for async operations
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    // Background update check (non-blocking, errors silently ignored)
    let update_handle = rt.spawn(update::check_for_update());

    // If API credentials provided, authenticate and fetch CID
    let (falcon_client, cid) = if has_api_creds {
        let client_id = cli.client_id.as_deref().unwrap();
        let client_secret = cli.client_secret.as_deref().unwrap();

        let result: Result<_, error::AppError> = rt.block_on(async {
            let client =
                falcon_api::FalconClient::new(client_id, client_secret, cli.cloud.as_deref())
                    .await?;

            let api_cid = if cli.cid.is_none() {
                Some(client.get_ccid().await?)
            } else {
                eprintln!("[init] Using explicit CID: {}", cli.cid.as_deref().unwrap());
                None
            };

            Ok((client, api_cid))
        });

        match result {
            Ok((client, api_cid)) => {
                let cid = cli.cid.unwrap_or_else(|| api_cid.unwrap());
                (Some(client), cid)
            }
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(1);
            }
        }
    } else {
        let cid = cli.cid.unwrap();
        eprintln!("[init] Local mode, CID: {cid}");
        (None, cid)
    };

    let state = Arc::new(AppState {
        token: token.clone(),
        cid,
        cloud: cli.cloud,
        addr: addr.clone(),
        port: cli.port,
        public_url: None,
        sensors: RwLock::new(sensors),
        download_count: AtomicU32::new(0),
        max_downloads: cli.max_downloads,
        shutdown_notify: shutdown_notify.clone(),
        falcon_client,
        hosts: Mutex::new(Vec::new()),
    });

    // Print banner
    let base_url = state.base_url();
    let max_dl_str = if cli.max_downloads == 0 {
        "unlimited".to_string()
    } else {
        cli.max_downloads.to_string()
    };

    eprintln!("pike — CrowdStrike sensor deployer");
    eprintln!("─────────────────────────────────────────");
    eprintln!("Server:    http://{}:{}", addr, cli.port);
    match &token {
        Some(t) => eprintln!("Token:     {}", t),
        None => eprintln!("Auth:      disabled"),
    }
    if state.falcon_client.is_some() {
        eprintln!("API:       connected");
    }
    let timeout_str = if cli.timeout == 0 {
        "none".to_string()
    } else {
        format!("{} min", cli.timeout)
    };
    eprintln!(
        "Timeout:   {} | Max downloads: {}",
        timeout_str, max_dl_str
    );

    let sensors_snapshot = rt.block_on(state.sensors.read());
    if !sensors_snapshot.is_empty() {
        eprintln!("Sensors:");
        for s in sensors_snapshot.iter() {
            eprintln!("  {} (SHA256: {})", s.filename, s.sha256);
        }
    } else if state.falcon_client.is_some() {
        eprintln!("Sensors:   on-demand via API");
    }
    drop(sensors_snapshot);

    // Show update notice if background check completed
    if update_handle.is_finished() {
        if let Ok(Ok(Some(info))) = rt.block_on(update_handle) {
            eprintln!(
                "Update available: {} -> {} (run 'pike update --apply' to install)",
                info.current_version, info.latest_version
            );
        }
    }

    eprintln!();

    // Always show commands if CID is available (scripts always served with callback flow)
    eprintln!("Linux:");
    eprintln!("  curl -fsS {}/lin | sudo bash", base_url);
    eprintln!();
    eprintln!("Windows (Run as Administrator):");
    eprintln!("  irm {}/win | iex", base_url);
    eprintln!();

    // Start server
    let bind_addr: std::net::SocketAddr = match format!("{}:{}", cli.bind, cli.port).parse() {
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
            cli.timeout,
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
