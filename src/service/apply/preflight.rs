use std::path::Path;

/// The conditions without which installing makes no sense. Checked before
/// any write, so that "half installed" never exists as a state.
pub(super) fn preflight() -> Result<(), String> {
    if !Path::new("/run/systemd/system").exists() {
        return Err("systemd not detected (/run/systemd/system is missing)".into());
    }
    if effective_uid().as_deref() != Some("0") {
        return Err("must be run as root (try: sudo pike service-install)".into());
    }
    Ok(())
}

/// geteuid via libc would need a dependency — read the process status
/// instead. The line reads `Uid:\t<real>\t<effective>\t<saved>\t<fs>`, and
/// the effective one is what matters: permissions are checked against it.
fn effective_uid() -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_effective_uid(&status)
}

fn parse_effective_uid(status: &str) -> Option<String> {
    status
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2))
        .map(|v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_effective_uid_not_the_real_one() {
        let status = "Name:\tpike\nUid:\t1000\t0\t0\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_effective_uid(status).as_deref(), Some("0"));
    }

    #[test]
    fn missing_uid_line_is_none() {
        assert_eq!(parse_effective_uid("Name:\tpike\n"), None);
    }
}
