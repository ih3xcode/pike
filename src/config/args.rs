use std::path::PathBuf;

/// `pike serve` arguments. All optional — the defaults live in
/// [`crate::config::resolve`], otherwise a clap default would be
/// indistinguishable from an explicitly passed flag and would always
/// beat the config file.
#[derive(Debug, Default, clap::Args)]
pub struct ServeArgs {
    /// Path to the config file (defaults to /etc/pike/pike.toml when present)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Address to listen on
    #[arg(long)]
    pub bind: Option<String>,

    /// HTTP port
    #[arg(long)]
    pub port: Option<u16>,

    /// Address to advertise in the one-liners (auto-detected when unset)
    #[arg(long)]
    pub addr: Option<String>,

    /// Public URL behind a reverse proxy, e.g. https://pike.lab.local
    #[arg(long)]
    pub public_url: Option<String>,

    /// Authentication token (generated when unset)
    #[arg(long, env = "PIKE_TOKEN", hide_env_values = true)]
    pub token: Option<String>,

    /// Timeout in minutes, 0 = no limit
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Sensor download limit, 0 = no limit
    #[arg(long)]
    pub max_downloads: Option<u32>,

    /// CrowdStrike API Client ID
    #[arg(long, env = "PIKE_CLIENT_ID", hide_env_values = true)]
    pub client_id: Option<String>,

    /// CrowdStrike API Client Secret
    #[arg(long, env = "PIKE_CLIENT_SECRET", hide_env_values = true)]
    pub client_secret: Option<String>,

    /// Cloud: us-1, us-2, eu-1, us-gov-1, us-gov-2
    #[arg(long)]
    pub cloud: Option<String>,

    /// CrowdStrike Customer ID
    #[arg(long, env = "PIKE_CID", hide_env_values = true)]
    pub cid: Option<String>,

    /// Sensor cache directory
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Sensor list time-to-live in minutes
    #[arg(long)]
    pub metadata_ttl: Option<u64>,

    /// Maximum cache size in bytes
    #[arg(long)]
    pub cache_max_bytes: Option<u64>,

    /// Grouping tags, comma-separated
    #[arg(long)]
    pub tags: Option<String>,

    /// Do not add the default deployment/pike tag
    #[arg(long)]
    pub no_default_tag: bool,

    /// Local sensor file; may be given more than once
    #[arg(long = "sensor")]
    pub sensors: Vec<PathBuf>,

    /// Disable token authentication
    #[arg(long)]
    pub no_auth: bool,
}

impl ServeArgs {
    /// Test-only parsing — exercises clap's logic including env vars.
    #[cfg(test)]
    pub fn parse_from_args(argv: &[&str]) -> Self {
        // `derive(Args)` gives no CommandFactory — build the command by hand
        use clap::{Args, FromArgMatches};
        let cmd = <Self as Args>::augment_args(clap::Command::new("pike"));
        let matches = cmd.get_matches_from(argv);
        Self::from_arg_matches(&matches).expect("valid args")
    }
}
