use crate::update;

pub fn run_update_command(apply: bool) -> Result<(), i32> {
    eprintln!("pike {} — checking for updates...", update::CURRENT_VERSION);

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
