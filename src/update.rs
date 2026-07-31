use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_API_URL: &str = "https://api.github.com/repos/ih3xcode/pike/releases/latest";
const USER_AGENT: &str = concat!("pike/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub release_url: String,
    pub asset_size: u64,
    pub checksum_url: Option<String>,
}

/// Приймає як голий hex, так і формат `sha256sum`: "<hex>  <filename>".
pub fn verify_asset(data: &[u8], expected: &str) -> Result<(), String> {
    let expected_hex = expected
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if expected_hex.len() != 64 || !expected_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("malformed checksum: {expected:?}"));
    }

    use sha2::{Digest, Sha256};
    let actual = hex::encode(Sha256::digest(data));
    if actual == expected_hex {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch: expected {expected_hex}, got {actual}"
        ))
    }
}

fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("pike-linux-amd64"),
        ("linux", "aarch64") => Some("pike-linux-arm64"),
        ("windows", "x86_64") => Some("pike-windows-amd64.exe"),
        _ => None,
    }
}

pub async fn check_for_update() -> Result<Option<UpdateInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let expected_asset = asset_name().ok_or("unsupported platform for updates")?;

    let client = reqwest::Client::new();
    let release: GitHubRelease = client
        .get(GITHUB_API_URL)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest_tag = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);
    let current = semver::Version::parse(CURRENT_VERSION)?;
    let latest = semver::Version::parse(latest_tag)?;

    if latest <= current {
        return Ok(None);
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == expected_asset)
        .ok_or_else(|| format!("no asset '{}' in release", expected_asset))?;

    let checksum_name = format!("{expected_asset}.sha256");
    let checksum_url = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .map(|a| a.browser_download_url.clone());

    Ok(Some(UpdateInfo {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: latest_tag.to_string(),
        download_url: asset.browser_download_url.clone(),
        release_url: release.html_url,
        asset_size: asset.size,
        checksum_url,
    }))
}

pub async fn apply_update(info: &UpdateInfo) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let bytes = client
        .get(&info.download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    match &info.checksum_url {
        Some(url) => {
            let checksum = client
                .get(url)
                .header("User-Agent", USER_AGENT)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            verify_asset(&bytes, &checksum)?;
            eprintln!("Checksum verified.");
        }
        None => {
            return Err(
                "release has no .sha256 asset — refusing to replace the binary unverified".into(),
            );
        }
    }

    // Write to a temp file, then replace the current binary
    let mut tmp = tempfile::NamedTempFile::new()?;
    std::io::Write::write_all(&mut tmp, &bytes)?;

    // On Unix, the downloaded binary needs to be executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755))?;
    }

    self_replace::self_replace(tmp.path())?;

    if std::path::Path::new("/etc/systemd/system/pike.service").exists() {
        eprintln!("A pike systemd service is installed — run: systemctl restart pike");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_valid_semver() {
        semver::Version::parse(CURRENT_VERSION).expect("CARGO_PKG_VERSION should be valid semver");
    }

    #[test]
    fn asset_name_returns_value() {
        // On any CI/dev machine (linux x86_64 or aarch64), we should get a name
        let name = asset_name();
        if cfg!(target_os = "linux") {
            assert!(name.is_some());
        }
    }

    #[test]
    fn verify_accepts_matching_checksum() {
        let data = b"binary contents";
        let sha = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(data);
            hex::encode(h.finalize())
        };
        assert!(verify_asset(data, &sha).is_ok());
    }

    #[test]
    fn verify_accepts_sha256sum_file_format() {
        // GNU sha256sum пише "<hex>  <filename>"
        let data = b"binary contents";
        let sha = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(data);
            hex::encode(h.finalize())
        };
        let line = format!("{sha}  pike-linux-amd64\n");
        assert!(verify_asset(data, &line).is_ok());
    }

    #[test]
    fn verify_rejects_mismatch() {
        assert!(verify_asset(b"actual", &"0".repeat(64)).is_err());
    }

    #[test]
    fn verify_rejects_garbage_checksum() {
        assert!(verify_asset(b"actual", "not a checksum").is_err());
    }

    #[test]
    fn version_comparison() {
        let older = semver::Version::parse("0.1.0").unwrap();
        let newer = semver::Version::parse("0.2.0").unwrap();
        assert!(newer > older);
        assert!(!(older > older));
    }
}
