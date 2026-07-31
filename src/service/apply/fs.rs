use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use super::cmd::run_cmd;
use super::{BIN_PATH, SERVICE_USER};

pub(super) fn ensure_user() -> Result<(), String> {
    let exists = Command::new("id")
        .arg(SERVICE_USER)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if exists {
        eprintln!("  user '{SERVICE_USER}' already exists");
        return Ok(());
    }
    // useradd rather than editing /etc/passwd: the latter breaks on systems
    // with SSSD, LDAP and other NSS backends
    run_cmd(
        "useradd",
        &[
            "--system",
            "--no-create-home",
            "--shell",
            "/usr/sbin/nologin",
            SERVICE_USER,
        ],
    )?;
    eprintln!("  created user '{SERVICE_USER}'");
    Ok(())
}

pub(super) fn install_binary() -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("cannot locate own binary: {e}"))?;
    if current == Path::new(BIN_PATH) {
        eprintln!("  binary already at {BIN_PATH}");
        return Ok(());
    }
    // Copying straight over a running service would give ETXTBSY, so write
    // alongside and rename: renaming over a running binary is allowed, and
    // the old inode lives until the process exits
    let staged = Path::new(BIN_PATH).with_extension("new");
    std::fs::copy(&current, &staged)
        .map_err(|e| format!("cannot stage binary at {}: {e}", staged.display()))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("cannot chmod {}: {e}", staged.display()))?;
    if let Err(e) = std::fs::rename(&staged, BIN_PATH) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("cannot install binary at {BIN_PATH}: {e}"));
    }
    eprintln!("  installed binary at {BIN_PATH}");
    Ok(())
}

pub(super) fn write_secure(path: &str, content: &str, mode: u32, owner: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("cannot write {path}: {e}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot chmod {path}: {e}"))?;
    run_cmd("chown", &[owner, path])?;
    Ok(())
}

/// Creates a directory with the given mode and owner.
pub(super) fn prepare_dir(path: &str, mode: u32, owner: &str) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {path}: {e}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot chmod {path}: {e}"))?;
    run_cmd("chown", &[owner, path])?;
    Ok(())
}

/// Removes a path, reporting failures without treating them as fatal.
pub(super) fn remove_best_effort(path: &Path) {
    if !path.exists() {
        return;
    }
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => eprintln!("  removed {}", path.display()),
        Err(e) => eprintln!("  WARNING: cannot remove {}: {e}", path.display()),
    }
}
