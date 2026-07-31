use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use super::units::{UnitParams, main_unit, update_timer, update_unit};
use super::wizard;

const BIN_PATH: &str = "/usr/local/bin/pike";
const CONFIG_DIR: &str = "/etc/pike";
const CONFIG_PATH: &str = "/etc/pike/pike.toml";
const UNIT_PATH: &str = "/etc/systemd/system/pike.service";
const UPDATE_UNIT_PATH: &str = "/etc/systemd/system/pike-update.service";
const UPDATE_TIMER_PATH: &str = "/etc/systemd/system/pike-update.timer";
const SERVICE_USER: &str = "pike";

fn run_cmd(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed with {status}", args.join(" ")))
    }
}

fn preflight() -> Result<(), String> {
    if !Path::new("/run/systemd/system").exists() {
        return Err("systemd not detected (/run/systemd/system is missing)".into());
    }
    // geteuid через libc недоступний без залежності — читаємо статус процесу
    let uid = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|v| v.to_string()))
        })
        .unwrap_or_default();
    if uid != "0" {
        return Err("must be run as root (try: sudo pike service-install)".into());
    }
    Ok(())
}

fn ensure_user() -> Result<(), String> {
    let exists = Command::new("id")
        .arg(SERVICE_USER)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if exists {
        eprintln!("  user '{SERVICE_USER}' already exists");
        return Ok(());
    }
    // useradd, а не редагування /etc/passwd: інакше зламаємось
    // на системах з SSSD, LDAP та іншими NSS-бекендами
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

fn install_binary() -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("cannot locate own binary: {e}"))?;
    if current == Path::new(BIN_PATH) {
        eprintln!("  binary already at {BIN_PATH}");
        return Ok(());
    }
    std::fs::copy(&current, BIN_PATH)
        .map_err(|e| format!("cannot copy binary to {BIN_PATH}: {e}"))?;
    std::fs::set_permissions(BIN_PATH, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("cannot chmod {BIN_PATH}: {e}"))?;
    eprintln!("  installed binary at {BIN_PATH}");
    Ok(())
}

fn write_secure(path: &str, content: &str, mode: u32, owner: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("cannot write {path}: {e}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot chmod {path}: {e}"))?;
    run_cmd("chown", &[owner, path])?;
    Ok(())
}

pub fn install() -> Result<(), i32> {
    if let Err(e) = preflight() {
        eprintln!("ERROR: {e}");
        return Err(1);
    }

    if Path::new(UNIT_PATH).exists() {
        eprintln!("A pike service is already installed at {UNIT_PATH}.");
        eprintln!("Re-running will overwrite its unit and config.");
        let mut line = String::new();
        eprint!("Continue? [y/N]: ");
        std::io::stdin().read_line(&mut line).ok();
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            eprintln!("Aborted.");
            return Err(1);
        }
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
    let result = (|| -> Result<(), String> {
        ensure_user()?;
        install_binary()?;

        std::fs::create_dir_all(CONFIG_DIR)
            .map_err(|e| format!("cannot create {CONFIG_DIR}: {e}"))?;
        std::fs::set_permissions(CONFIG_DIR, std::fs::Permissions::from_mode(0o750))
            .map_err(|e| format!("cannot chmod {CONFIG_DIR}: {e}"))?;
        run_cmd("chown", &[&format!("root:{SERVICE_USER}"), CONFIG_DIR])?;

        write_secure(
            CONFIG_PATH,
            &plan.config_toml,
            0o640,
            &format!("root:{SERVICE_USER}"),
        )?;
        eprintln!("  wrote {CONFIG_PATH}");

        std::fs::create_dir_all(&plan.cache_dir)
            .map_err(|e| format!("cannot create {}: {e}", plan.cache_dir))?;
        std::fs::set_permissions(&plan.cache_dir, std::fs::Permissions::from_mode(0o750))
            .map_err(|e| format!("cannot chmod {}: {e}", plan.cache_dir))?;
        run_cmd(
            "chown",
            &[&format!("{SERVICE_USER}:{SERVICE_USER}"), &plan.cache_dir],
        )?;
        eprintln!("  prepared cache dir {}", plan.cache_dir);

        let unit = main_unit(&UnitParams {
            exec_path: BIN_PATH.into(),
            config_path: CONFIG_PATH.into(),
            cache_dir: plan.cache_dir.clone(),
            user: SERVICE_USER.into(),
            port: plan.port,
        });
        std::fs::write(UNIT_PATH, unit).map_err(|e| format!("cannot write {UNIT_PATH}: {e}"))?;
        eprintln!("  wrote {UNIT_PATH}");

        if plan.enable_auto_update {
            std::fs::write(UPDATE_UNIT_PATH, update_unit(BIN_PATH))
                .map_err(|e| format!("cannot write {UPDATE_UNIT_PATH}: {e}"))?;
            std::fs::write(UPDATE_TIMER_PATH, update_timer())
                .map_err(|e| format!("cannot write {UPDATE_TIMER_PATH}: {e}"))?;
            eprintln!("  wrote auto-update timer units");
        }

        run_cmd("systemctl", &["daemon-reload"])?;
        run_cmd("systemctl", &["enable", "--now", "pike.service"])?;
        if plan.enable_auto_update {
            run_cmd("systemctl", &["enable", "--now", "pike-update.timer"])?;
        }
        Ok(())
    })();

    if let Err(e) = result {
        eprintln!("ERROR: {e}");
        eprintln!("The system may be partially configured; run 'pike service-uninstall' to clean up.");
        return Err(1);
    }

    let active = Command::new("systemctl")
        .args(["is-active", "--quiet", "pike.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    eprintln!("\n──────────────────────");
    if active {
        eprintln!("pike.service is running.\n");
        eprintln!("Linux:");
        eprintln!("  curl -fsS {}/lin | sudo bash\n", plan.base_url_hint);
        eprintln!("Windows (Run as Administrator):");
        eprintln!("  irm {}/win | iex\n", plan.base_url_hint);
        eprintln!("Logs: journalctl -u pike -f");
        Ok(())
    } else {
        eprintln!("pike.service did NOT start. Check: journalctl -u pike -n 50");
        Err(1)
    }
}

pub fn uninstall(_purge: bool) -> Result<(), i32> {
    eprintln!("ERROR: not implemented yet");
    Err(1)
}
