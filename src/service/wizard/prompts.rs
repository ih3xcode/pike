use std::io::{self, Write};

/// Every prompt fails rather than loops when stdin is exhausted. A question
/// with no default used to spin forever on `pike service-install < /dev/null`
/// — a busy loop that pegged a CPU and flooded stderr.
const EOF: &str = "stdin is closed — 'pike service-install' needs an interactive terminal";

fn read_answer() -> Result<Option<String>, String> {
    let mut line = String::new();
    let read = io::stdin().read_line(&mut line);
    interpret(read, &line)
}

/// `Ok(0)` from `read_line` means end of input, not "an empty answer" —
/// telling the two apart is what stops a required question looping forever.
fn interpret(read: io::Result<usize>, line: &str) -> Result<Option<String>, String> {
    match read {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(line.trim().to_string())),
        Err(e) => Err(format!("cannot read from stdin: {e}")),
    }
}

pub(super) fn prompt(label: &str, default: Option<&str>) -> Result<String, String> {
    loop {
        match default {
            Some(d) => print!("{label} [{d}]: "),
            None => print!("{label}: "),
        }
        io::stdout().flush().ok();

        let Some(line) = read_answer()? else {
            return Err(EOF.into());
        };
        if !line.is_empty() {
            return Ok(line);
        }
        if let Some(d) = default {
            return Ok(d.to_string());
        }
        eprintln!("  (this one is required)");
    }
}

pub(super) fn prompt_optional(label: &str) -> Result<Option<String>, String> {
    let v = prompt_allow_empty(label)?;
    Ok(if v.is_empty() { None } else { Some(v) })
}

pub(super) fn prompt_allow_empty(label: &str) -> Result<String, String> {
    print!("{label} (leave empty to skip): ");
    io::stdout().flush().ok();
    read_answer()?.ok_or_else(|| EOF.to_string())
}

/// Asks until the answer passes validation. The wizard promises everything
/// is checked before the first write to disk — otherwise a bad token or bind
/// address would only surface from `systemctl`, once the service is installed.
pub(super) fn prompt_valid<T>(
    label: &str,
    default: Option<&str>,
    validate: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    loop {
        let raw = prompt(label, default)?;
        match validate(&raw) {
            Ok(v) => return Ok(v),
            Err(e) => eprintln!("  ({e})"),
        }
    }
}

pub(super) fn prompt_bool(label: &str, default: bool) -> Result<bool, String> {
    let hint = if default { "Y/n" } else { "y/N" };
    loop {
        print!("{label} [{hint}]: ");
        io::stdout().flush().ok();

        let Some(line) = read_answer()? else {
            return Err(EOF.into());
        };
        match line.to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => eprintln!("  (answer y or n)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_of_input_is_not_an_empty_answer() {
        assert_eq!(interpret(Ok(0), ""), Ok(None));
    }

    #[test]
    fn a_blank_line_is_an_empty_answer() {
        assert_eq!(interpret(Ok(1), "\n"), Ok(Some(String::new())));
    }

    #[test]
    fn an_answer_is_trimmed() {
        assert_eq!(interpret(Ok(6), "  hi \n"), Ok(Some("hi".into())));
    }

    #[test]
    fn a_read_error_is_reported() {
        let err = io::Error::other("broken pipe");
        assert!(interpret(Err(err), "").is_err());
    }
}
