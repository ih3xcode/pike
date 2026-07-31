use crate::sensors::SensorType;

/// A parsed callback: `hostname|pkg_type|arch|distro_id|distro_version`.
/// The last two fields are optional — Windows does not send them.
pub struct CallbackInfo {
    pub hostname: String,
    pub pkg_type: String,
    pub arch: String,
    pub distro_id: String,
    pub distro_version: String,
    pub target_type: SensorType,
}

pub(super) fn is_valid_hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn is_valid_arch(s: &str) -> bool {
    matches!(
        s,
        "x86_64" | "amd64" | "AMD64" | "aarch64" | "arm64" | "s390x" | "ppc64le"
    )
}

fn is_valid_distro_field(s: &str) -> bool {
    s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn parse_callback(body: &str) -> Result<CallbackInfo, &'static str> {
    let parts: Vec<&str> = body.trim().splitn(5, '|').collect();
    if parts.len() < 3 {
        return Err("invalid callback format");
    }

    let hostname = parts[0];
    if !is_valid_hostname(hostname) {
        return Err("invalid hostname");
    }

    let arch = parts[2];
    if !is_valid_arch(arch) {
        return Err("unsupported architecture");
    }

    let pkg_type = parts[1];
    let target_type = match pkg_type {
        "deb" => SensorType::Deb,
        "rpm" => SensorType::Rpm,
        "exe" => SensorType::WindowsExe,
        _ => return Err("unsupported package type"),
    };

    let distro_id = if parts.len() > 3 { parts[3] } else { "" };
    let distro_version = if parts.len() > 4 { parts[4] } else { "" };

    if !is_valid_distro_field(distro_id) || !is_valid_distro_field(distro_version) {
        return Err("invalid distro field");
    }

    Ok(CallbackInfo {
        hostname: hostname.to_string(),
        pkg_type: pkg_type.to_string(),
        arch: arch.to_string(),
        distro_id: distro_id.to_string(),
        distro_version: distro_version.to_string(),
        target_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_valid_hostname ---

    #[test]
    fn hostname_valid() {
        assert!(is_valid_hostname("web-server.example.com"));
        assert!(is_valid_hostname("host_01"));
        assert!(is_valid_hostname("a"));
    }

    #[test]
    fn hostname_empty() {
        assert!(!is_valid_hostname(""));
    }

    #[test]
    fn hostname_too_long() {
        let long = "a".repeat(254);
        assert!(!is_valid_hostname(&long));
    }

    #[test]
    fn hostname_253_ok() {
        let exact = "a".repeat(253);
        assert!(is_valid_hostname(&exact));
    }

    #[test]
    fn hostname_special_chars() {
        assert!(!is_valid_hostname("host;rm -rf /"));
        assert!(!is_valid_hostname("host name"));
        assert!(!is_valid_hostname("host\nname"));
    }

    #[test]
    fn hostname_unicode() {
        assert!(!is_valid_hostname("höst"));
    }

    // --- is_valid_arch ---

    #[test]
    fn arch_valid_all() {
        for arch in &[
            "x86_64", "amd64", "AMD64", "aarch64", "arm64", "s390x", "ppc64le",
        ] {
            assert!(is_valid_arch(arch), "{arch} should be valid");
        }
    }

    #[test]
    fn arch_invalid() {
        assert!(!is_valid_arch("i386"));
        assert!(!is_valid_arch("i686"));
        assert!(!is_valid_arch(""));
        assert!(!is_valid_arch("x86"));
    }

    // --- is_valid_distro_field ---

    #[test]
    fn distro_field_valid() {
        assert!(is_valid_distro_field("ubuntu"));
        assert!(is_valid_distro_field("22.04"));
        assert!(is_valid_distro_field("opensuse-leap"));
        assert!(is_valid_distro_field("")); // empty is valid (optional field)
    }

    #[test]
    fn distro_field_too_long() {
        let long = "a".repeat(65);
        assert!(!is_valid_distro_field(&long));
    }

    #[test]
    fn distro_field_special_chars() {
        assert!(!is_valid_distro_field("ubuntu; rm -rf /"));
        assert!(!is_valid_distro_field("distro\nid"));
    }

    // --- parse_callback ---

    #[test]
    fn parse_callback_valid_5_fields() {
        let result = parse_callback("myhost|deb|x86_64|ubuntu|22.04").unwrap();
        assert_eq!(result.hostname, "myhost");
        assert_eq!(result.pkg_type, "deb");
        assert_eq!(result.arch, "x86_64");
        assert_eq!(result.distro_id, "ubuntu");
        assert_eq!(result.distro_version, "22.04");
        assert_eq!(result.target_type, SensorType::Deb);
    }

    #[test]
    fn parse_callback_valid_3_fields() {
        let result = parse_callback("winhost|exe|AMD64").unwrap();
        assert_eq!(result.hostname, "winhost");
        assert_eq!(result.pkg_type, "exe");
        assert_eq!(result.arch, "AMD64");
        assert_eq!(result.distro_id, "");
        assert_eq!(result.distro_version, "");
        assert_eq!(result.target_type, SensorType::WindowsExe);
    }

    #[test]
    fn parse_callback_rpm() {
        let result = parse_callback("rhelbox|rpm|aarch64|rhel|9.2").unwrap();
        assert_eq!(result.target_type, SensorType::Rpm);
    }

    #[test]
    fn parse_callback_too_few_fields() {
        assert!(parse_callback("host|deb").is_err());
        assert!(parse_callback("host").is_err());
        assert!(parse_callback("").is_err());
    }

    #[test]
    fn parse_callback_bad_hostname() {
        assert!(parse_callback("|deb|x86_64").is_err());
        assert!(parse_callback("host;evil|deb|x86_64").is_err());
    }

    #[test]
    fn parse_callback_bad_arch() {
        assert!(parse_callback("host|deb|i386").is_err());
    }

    #[test]
    fn parse_callback_bad_pkg_type() {
        assert!(parse_callback("host|msi|x86_64").is_err());
    }

    #[test]
    fn parse_callback_trims_whitespace() {
        let result = parse_callback("  myhost|deb|x86_64|ubuntu|22.04\n").unwrap();
        assert_eq!(result.hostname, "myhost");
    }
}
