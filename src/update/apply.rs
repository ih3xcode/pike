use super::check::{UpdateInfo, USER_AGENT};
use super::verify::verify_asset;

pub async fn apply_update(
    info: &UpdateInfo,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
