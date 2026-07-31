use std::io::{self, Write};

pub(super) fn prompt(label: &str, default: Option<&str>) -> String {
    loop {
        match default {
            Some(d) => print!("{label} [{d}]: "),
            None => print!("{label}: "),
        }
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            continue;
        }
        let line = line.trim().to_string();
        if !line.is_empty() {
            return line;
        }
        if let Some(d) = default {
            return d.to_string();
        }
        eprintln!("  (this one is required)");
    }
}

pub(super) fn prompt_optional(label: &str) -> Option<String> {
    let v = prompt_allow_empty(label);
    if v.is_empty() { None } else { Some(v) }
}

pub(super) fn prompt_allow_empty(label: &str) -> String {
    print!("{label} (leave empty to skip): ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line).ok();
    line.trim().to_string()
}

/// Asks until the answer passes validation. The wizard promises everything
/// is checked before the first write to disk — otherwise a bad token or bind
/// address would only surface from `systemctl`, once the service is installed.
pub(super) fn prompt_valid<T>(
    label: &str,
    default: Option<&str>,
    validate: impl Fn(&str) -> Result<T, String>,
) -> T {
    loop {
        let raw = prompt(label, default);
        match validate(&raw) {
            Ok(v) => return v,
            Err(e) => eprintln!("  ({e})"),
        }
    }
}

pub(super) fn prompt_bool(label: &str, default: bool) -> bool {
    let hint = if default { "Y/n" } else { "y/N" };
    loop {
        print!("{label} [{hint}]: ");
        io::stdout().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok();
        match line.trim().to_lowercase().as_str() {
            "" => return default,
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => eprintln!("  (answer y or n)"),
        }
    }
}
