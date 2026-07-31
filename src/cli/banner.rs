use crate::config::ResolvedConfig;
use crate::server::AppState;

/// What the operator sees right after startup: addresses, limits and the
/// two ready-to-paste one-liners.
pub fn print_banner(
    state: &AppState,
    cfg: &ResolvedConfig,
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
