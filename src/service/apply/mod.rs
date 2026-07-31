//! Виконання плану установки: користувач, бінар, конфіг, кеш, юніти.
//!
//! Усе, що могло провалитись через відповіді оператора, вже перевірив
//! візард — тут лишились тільки операції над системою.

mod cmd;
mod fs;
mod preflight;
mod systemd;

use std::path::Path;

use super::units::UnitParams;
use super::wizard;

use preflight::preflight;

const BIN_PATH: &str = "/usr/local/bin/pike";
const CONFIG_DIR: &str = "/etc/pike";
const CONFIG_PATH: &str = "/etc/pike/pike.toml";
const SERVICE_USER: &str = "pike";

pub fn install() -> Result<(), i32> {
    if let Err(e) = preflight() {
        eprintln!("ERROR: {e}");
        return Err(1);
    }

    if Path::new(systemd::UNIT_PATH).exists() && !confirm_overwrite() {
        eprintln!("Aborted.");
        return Err(1);
    }

    // Усе, що може провалитись, провалюється тут — до першого запису
    let plan = match wizard::run() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return Err(1);
        }
    };

    eprintln!("\nInstalling...");
    if let Err(e) = write_everything(&plan) {
        eprintln!("ERROR: {e}");
        eprintln!("The system may be partially configured; run 'pike service-uninstall' to clean up.");
        return Err(1);
    }

    report(&plan)
}

fn confirm_overwrite() -> bool {
    eprintln!(
        "A pike service is already installed at {}.",
        systemd::UNIT_PATH
    );
    eprintln!("Re-running will overwrite its unit and config.");
    let mut line = String::new();
    eprint!("Continue? [y/N]: ");
    std::io::stdin().read_line(&mut line).ok();
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn write_everything(plan: &wizard::InstallPlan) -> Result<(), String> {
    fs::ensure_user()?;
    fs::install_binary()?;

    let root_group = format!("root:{SERVICE_USER}");
    fs::prepare_dir(CONFIG_DIR, 0o750, &root_group)?;
    fs::write_secure(CONFIG_PATH, &plan.config_toml, 0o640, &root_group)?;
    eprintln!("  wrote {CONFIG_PATH}");

    fs::prepare_dir(
        &plan.cache_dir,
        0o750,
        &format!("{SERVICE_USER}:{SERVICE_USER}"),
    )?;
    eprintln!("  prepared cache dir {}", plan.cache_dir);

    systemd::write_main_unit(&UnitParams {
        exec_path: BIN_PATH.into(),
        config_path: CONFIG_PATH.into(),
        cache_dir: plan.cache_dir.clone(),
        user: SERVICE_USER.into(),
        port: plan.port,
    })?;

    if plan.enable_auto_update {
        systemd::write_update_units()?;
    } else {
        // Переустановка з відмовою від автооновлення має вимкнути таймер,
        // що лишився з попереднього разу — інакше root і далі щотижня
        // переписував би бінар усупереч щойно висловленій волі
        systemd::remove_update_units();
    }

    systemd::daemon_reload()?;
    systemd::enable_now("pike.service")?;
    if plan.enable_auto_update {
        systemd::enable_now("pike-update.timer")?;
    }
    Ok(())
}

fn report(plan: &wizard::InstallPlan) -> Result<(), i32> {
    eprintln!("\n──────────────────────");
    if !systemd::is_active("pike.service") {
        eprintln!("pike.service did NOT start. Check: journalctl -u pike -n 50");
        return Err(1);
    }

    eprintln!("pike.service is running.\n");
    eprintln!("Linux:");
    eprintln!("  curl -fsS {}/lin | sudo bash\n", plan.base_url_hint);
    eprintln!("Windows (Run as Administrator):");
    eprintln!("  irm {}/win | iex\n", plan.base_url_hint);
    eprintln!("Logs: journalctl -u pike -f");
    Ok(())
}

pub fn uninstall(purge: bool) -> Result<(), i32> {
    if let Err(e) = preflight() {
        eprintln!("ERROR: {e}");
        return Err(1);
    }

    // Помилки тут не фатальні: юніта може вже не бути,
    // а мета команди — привести систему в чистий стан.
    for unit in ["pike.service", "pike-update.timer"] {
        systemd::disable_now(unit);
    }
    for path in [
        systemd::UNIT_PATH,
        systemd::UPDATE_UNIT_PATH,
        systemd::UPDATE_TIMER_PATH,
    ] {
        fs::remove_best_effort(Path::new(path));
    }
    let _ = systemd::daemon_reload();

    if !purge {
        eprintln!("\nService removed. Config, cache and the '{SERVICE_USER}' user were kept.");
        eprintln!("Run with --purge to remove them as well.");
        return Ok(());
    }

    // Конфіг містить секрети — його видалення має бути свідомим,
    // тому воно живе тільки за явним --purge
    let cache_dir = configured_cache_dir();
    fs::remove_best_effort(Path::new(CONFIG_DIR));
    fs::remove_best_effort(&cache_dir);

    let _ = std::process::Command::new("userdel")
        .arg(SERVICE_USER)
        .status();
    eprintln!("  removed user '{SERVICE_USER}'");

    eprintln!("\nPurged. The binary at {BIN_PATH} was left in place.");
    Ok(())
}

/// Каталог кешу з чинного конфігу; типовий сервісний шлях, якщо конфіг
/// уже зник або не читається.
fn configured_cache_dir() -> std::path::PathBuf {
    std::fs::read_to_string(CONFIG_PATH)
        .ok()
        .and_then(|text| toml::from_str::<crate::config::FileConfig>(&text).ok())
        .and_then(|c| c.sensors.cache_dir)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(crate::config::defaults::DEFAULT_SERVICE_CACHE_DIR)
        })
}
