use std::path::Path;

/// Умови, без яких установка не має сенсу. Перевіряються до будь-якого
/// запису — щоб «встановлено наполовину» не існувало як стан.
pub(super) fn preflight() -> Result<(), String> {
    if !Path::new("/run/systemd/system").exists() {
        return Err("systemd not detected (/run/systemd/system is missing)".into());
    }
    if effective_uid().as_deref() != Some("0") {
        return Err("must be run as root (try: sudo pike service-install)".into());
    }
    Ok(())
}

/// geteuid через libc недоступний без залежності — читаємо статус процесу.
fn effective_uid() -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|v| v.to_string())
}
