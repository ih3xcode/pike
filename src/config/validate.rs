use crate::common::AppError;

/// Cloud regions pike knows. Every entry must have a base URL in
/// `crate::falcon::auth::api_base_url` — a test over there checks that.
pub const CLOUDS: &[&str] = &["us-1", "us-2", "eu-1", "us-gov-1", "us-gov-2"];

/// An unknown region is a hard error rather than a fallback. `api_base_url`
/// answers with us-1 for anything it does not recognise and the install script
/// drops its `--cloud` argument, so a typo would quietly enrol every host in
/// the wrong region instead of failing.
pub fn validate_cloud(cloud: &str) -> Result<(), AppError> {
    if CLOUDS.contains(&cloud) {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Unknown cloud '{cloud}': expected one of {}",
            CLOUDS.join(", ")
        )))
    }
}

/// The token ends up in a URL path — only characters safe there are allowed.
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

    #[test]
    fn accepts_every_known_cloud() {
        for cloud in CLOUDS {
            assert!(validate_cloud(cloud).is_ok(), "{cloud} should be accepted");
        }
    }

    #[test]
    fn rejects_a_mistyped_cloud() {
        // 'eu1' used to authenticate against us-1 and drop --cloud from the
        // install script, enrolling every host in the wrong region
        assert!(validate_cloud("eu1").is_err());
        assert!(validate_cloud("EU-1").is_err());
        assert!(validate_cloud("").is_err());
    }
}
