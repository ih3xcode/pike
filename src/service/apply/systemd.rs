use std::path::Path;
use std::process::Command;

use crate::service::units::{main_unit, update_timer, update_unit, UnitParams};

use super::cmd::run_cmd;
use super::fs::remove_best_effort;
use super::BIN_PATH;

pub(super) const UNIT_PATH: &str = "/etc/systemd/system/pike.service";
pub(super) const UPDATE_UNIT_PATH: &str = "/etc/systemd/system/pike-update.service";
pub(super) const UPDATE_TIMER_PATH: &str = "/etc/systemd/system/pike-update.timer";

pub(super) fn write_main_unit(params: &UnitParams) -> Result<(), String> {
    std::fs::write(UNIT_PATH, main_unit(params))
        .map_err(|e| format!("cannot write {UNIT_PATH}: {e}"))?;
    eprintln!("  wrote {UNIT_PATH}");
    Ok(())
}

pub(super) fn write_update_units() -> Result<(), String> {
    std::fs::write(UPDATE_UNIT_PATH, update_unit(BIN_PATH))
        .map_err(|e| format!("cannot write {UPDATE_UNIT_PATH}: {e}"))?;
    std::fs::write(UPDATE_TIMER_PATH, update_timer())
        .map_err(|e| format!("cannot write {UPDATE_TIMER_PATH}: {e}"))?;
    eprintln!("  wrote auto-update timer units");
    Ok(())
}

/// Вимикає й видаляє юніти автооновлення. Помилки не фатальні — юнітів
/// може просто не бути.
pub(super) fn remove_update_units() {
    let existed = Path::new(UPDATE_TIMER_PATH).exists() || Path::new(UPDATE_UNIT_PATH).exists();
    disable_now("pike-update.timer");
    for path in [UPDATE_TIMER_PATH, UPDATE_UNIT_PATH] {
        remove_best_effort(Path::new(path));
    }
    if existed {
        eprintln!("  auto-update timer disabled");
    }
}

pub(super) fn daemon_reload() -> Result<(), String> {
    run_cmd("systemctl", &["daemon-reload"])
}

pub(super) fn enable_now(unit: &str) -> Result<(), String> {
    run_cmd("systemctl", &["enable", "--now", unit])
}

/// Вимикає юніт, ігноруючи помилки: мета — привести систему в чистий стан.
pub(super) fn disable_now(unit: &str) {
    let _ = Command::new("systemctl")
        .args(["disable", "--now", unit])
        .status();
}

pub(super) fn is_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
