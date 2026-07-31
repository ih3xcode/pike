//! Interactive collection of the answers for `pike service-install`.
//!
//! The wizard writes nothing to disk: everything that can fail — validating
//! credentials, parsing addresses, checking the token — happens here, before
//! the first write. The result is a ready [`InstallPlan`].

mod prompts;
mod render;
mod validate;

use crate::config::defaults;
use crate::falcon::FalconClient;
use crate::sensors::ports::SensorLister;

use prompts::{prompt, prompt_allow_empty, prompt_bool, prompt_optional, prompt_valid};
use render::render_config;
use validate::{validate_bind, validate_cache_dir};

pub struct Answers {
    pub client_id: String,
    pub client_secret: String,
    pub cloud: String,
    pub cid: String,
    pub port: u16,
    pub bind: String,
    pub addr: Option<String>,
    pub public_url: Option<String>,
    pub token: Option<String>,
    pub tags: Option<String>,
    pub cache_dir: String,
    pub metadata_ttl_minutes: u64,
    pub cache_max_bytes: u64,
}

pub struct InstallPlan {
    pub config_toml: String,
    pub port: u16,
    pub cache_dir: String,
    pub enable_auto_update: bool,
    pub base_url_hint: String,
}

pub fn run() -> Result<InstallPlan, String> {
    eprintln!("pike service installer");
    eprintln!("──────────────────────\n");

    let cloud = prompt(
        "CrowdStrike cloud (us-1/us-2/eu-1/us-gov-1/us-gov-2)",
        Some("eu-1"),
    );
    let client_id = prompt("API Client ID", None);
    let client_secret =
        rpassword::prompt_password("API Client Secret: ").map_err(|e| e.to_string())?;

    let api_cid = validate_credentials(&client_id, &client_secret, &cloud)?;
    eprintln!("  OK — CID {api_cid}\n");

    let cid = prompt("Customer ID", Some(&api_cid));

    let port: u16 = loop {
        let raw = prompt("HTTP port", Some("8080"));
        match raw.parse() {
            Ok(p) => break p,
            Err(_) => eprintln!("  (not a valid port)"),
        }
    };
    let bind = prompt_valid("Bind address", Some("0.0.0.0"), |v| validate_bind(v, port));
    let addr = ask_advertised_addr();

    let public_url =
        prompt_optional("Public URL behind a reverse proxy, e.g. https://pike.lab.local");

    let generated = crate::common::token::generate_token();
    eprintln!("\nGenerated token: {generated}");
    let token = Some(prompt_valid("Token", Some(&generated), |v| {
        crate::config::validate_token(v)
            .map(|()| v.to_string())
            .map_err(|e| e.to_string())
    }));

    let tags = prompt_optional("Grouping tags, comma-separated");
    let cache_dir = prompt_valid(
        "Cache directory",
        Some(defaults::DEFAULT_SERVICE_CACHE_DIR),
        validate_cache_dir,
    );
    let enable_auto_update = prompt_bool(
        "\nEnable the weekly auto-update timer? (pike is 0.x and may introduce breaking changes)",
        false,
    );

    let answers = Answers {
        client_id,
        client_secret,
        cloud,
        cid,
        port,
        bind,
        addr: addr.clone(),
        public_url: public_url.clone(),
        token: token.clone(),
        tags,
        cache_dir: cache_dir.clone(),
        metadata_ttl_minutes: defaults::DEFAULT_METADATA_TTL_MINUTES,
        cache_max_bytes: defaults::DEFAULT_CACHE_MAX_BYTES,
    };

    let host = public_url.unwrap_or_else(|| {
        format!(
            "http://{}:{}",
            addr.unwrap_or_else(|| "127.0.0.1".into()),
            port
        )
    });
    let base_url_hint = match &token {
        Some(t) => format!("{host}/{t}"),
        None => host,
    };

    Ok(InstallPlan {
        config_toml: render_config(&answers),
        port,
        cache_dir,
        enable_auto_update,
        base_url_hint,
    })
}

/// Validates the credentials against the API and returns the CID. Besides
/// authenticating it also lists sensors — the one scope without which the
/// server is useless.
fn validate_credentials(
    client_id: &str,
    client_secret: &str,
    cloud: &str,
) -> Result<String, String> {
    eprintln!("\nValidating credentials against the API...");
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let client = FalconClient::new(client_id, client_secret, Some(cloud))
            .await
            .map_err(|e| format!("authentication failed: {e}"))?;
        let cid = client
            .get_ccid()
            .await
            .map_err(|e| format!("cannot fetch CCID: {e}"))?;
        client.list("linux").await.map_err(|e| {
            format!("sensor listing failed — is the 'Sensor Download: Read' scope granted? ({e})")
        })?;
        Ok(cid)
    })
}

fn ask_advertised_addr() -> Option<String> {
    eprintln!("\nDetected addresses:");
    let addrs = crate::common::net::detect_available_addrs();
    for (i, (label, _)) in addrs.iter().enumerate() {
        eprintln!("  {i}) {label}");
    }
    loop {
        let raw = prompt_allow_empty("Advertised address — index or literal");
        if raw.is_empty() {
            return None;
        }
        // A number outside the list is almost certainly a mistyped index
        // rather than an address; storing it literally would hand hosts a
        // one-liner like http://4:8080/<token>
        match raw.parse::<usize>() {
            Ok(i) if i < addrs.len() => return Some(addrs[i].1.clone()),
            Ok(_) => eprintln!("  (no such index; pick 0-{})", addrs.len() - 1),
            Err(_) => return Some(raw),
        }
    }
}
