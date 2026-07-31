use crate::update;

pub fn run_update_command(apply: bool, restart_service: bool) -> Result<(), i32> {
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
            eprintln!("Successfully updated to pike {}.", info.latest_version);
            // Only reached when the binary was actually replaced, which is why
            // the restart lives here rather than in the unit's ExecStartPost=
            if restart_service {
                restart_pike_service();
            } else {
                eprintln!("Restart pike to use the new version.");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: Failed to apply update: {e}");
            Err(1)
        }
    }
}

#[cfg(target_os = "linux")]
fn restart_pike_service() {
    // try-restart, not restart: a host that only uses the CLI has no service
    // to bounce, and starting one it never asked for would be a surprise
    match std::process::Command::new("systemctl")
        .args(["try-restart", "pike.service"])
        .status()
    {
        Ok(s) if s.success() => eprintln!("Restarted pike.service."),
        Ok(s) => eprintln!("WARNING: 'systemctl try-restart pike.service' exited with {s}"),
        Err(e) => eprintln!("WARNING: cannot run systemctl: {e}"),
    }
}

#[cfg(not(target_os = "linux"))]
fn restart_pike_service() {
    eprintln!("--restart-service is only supported on Linux with systemd.");
}
