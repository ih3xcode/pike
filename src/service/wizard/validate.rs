pub(super) fn validate_bind(value: &str, port: u16) -> Result<String, String> {
    // Перевіряємо рівно те, що робитиме сервер на старті
    format!("{value}:{port}")
        .parse::<std::net::SocketAddr>()
        .map(|_| value.to_string())
        .map_err(|_| format!("'{value}' is not an IP address pike can bind to"))
}

pub(super) fn validate_cache_dir(value: &str) -> Result<String, String> {
    if !value.starts_with('/') {
        return Err("must be an absolute path".into());
    }
    // Пробіл розділив би значення в ReadWritePaths= і зламав юніт
    if value.split_whitespace().count() != 1 {
        return Err("must not contain whitespace".into());
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_accepts_addresses_the_server_can_parse() {
        assert!(validate_bind("0.0.0.0", 8080).is_ok());
        assert!(validate_bind("127.0.0.1", 8080).is_ok());
    }

    #[test]
    fn bind_rejects_hostnames() {
        // `localhost` пройшов би візард, але впав би на старті сервера
        assert!(validate_bind("localhost", 8080).is_err());
        assert!(validate_bind("", 8080).is_err());
    }

    #[test]
    fn cache_dir_must_be_absolute_and_space_free() {
        assert!(validate_cache_dir("/var/cache/pike").is_ok());
        assert!(validate_cache_dir("var/cache/pike").is_err());
        // Пробіл розділив би значення в ReadWritePaths=
        assert!(validate_cache_dir("/var/cache/my pike").is_err());
    }

    #[test]
    fn wizard_rejects_tokens_the_config_would_refuse() {
        assert!(crate::config::validate_token("lab/token").is_err());
        assert!(crate::config::validate_token("labtoken01").is_ok());
    }
}
