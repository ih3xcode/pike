use crate::common::AppError;

/// Токен потрапляє в шлях URL — дозволені лише безпечні там символи.
pub fn validate_token(token: &str) -> Result<(), AppError> {
    let ok = !token.is_empty()
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Invalid token '{token}': use 1-128 chars from [A-Za-z0-9_-]"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_url_safe_tokens() {
        assert!(validate_token("labtoken01").is_ok());
        assert!(validate_token("a-b_c").is_ok());
    }

    #[test]
    fn rejects_path_separators_and_empty() {
        assert!(validate_token("bad/token").is_err());
        assert!(validate_token("").is_err());
        assert!(validate_token(&"a".repeat(129)).is_err());
    }
}
