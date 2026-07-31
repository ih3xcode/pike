use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::common::error::AppError;

/// Timeouts for metadata calls. Downloading a sensor deliberately has no
/// total timeout — it is hundreds of megabytes that take a long time on a
/// slow link; only the connect timeout guards it.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn api_base_url(cloud: Option<&str>) -> &str {
    match cloud {
        Some("us-2") => "https://api.us-2.crowdstrike.com",
        Some("eu-1") => "https://api.eu-1.crowdstrike.com",
        Some("us-gov-1") => "https://api.laggar.gcw.crowdstrike.com",
        Some("us-gov-2") => "https://api.us-gov-2.crowdstrike.com",
        _ => "https://api.crowdstrike.com",
    }
}

#[derive(Deserialize)]
struct OAuthResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_expires_in() -> u64 {
    1799
}

struct TokenState {
    access_token: String,
    expires_at: Instant,
}

/// Holds the base URL, the credentials and the current token. Refreshes it
/// on its own 60 seconds before it actually expires, so endpoint calls never
/// have to think about the token's lifetime.
pub(super) struct Authenticator {
    base_url: String,
    client_id: String,
    client_secret: String,
    token: RwLock<TokenState>,
}

impl Authenticator {
    pub(super) async fn new(
        http: &reqwest::Client,
        base_url: String,
        client_id: &str,
        client_secret: &str,
    ) -> Result<Self, AppError> {
        eprintln!("[falcon] Authenticating to {base_url} ...");
        let (access_token, expires_at) =
            do_oauth2(http, &base_url, client_id, client_secret).await?;
        eprintln!("[falcon] Authenticated successfully");

        Ok(Self {
            base_url,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            token: RwLock::new(TokenState {
                access_token,
                expires_at,
            }),
        })
    }

    pub(super) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// A valid access token, refreshed when the current one has expired.
    pub(super) async fn access_token(&self, http: &reqwest::Client) -> Result<String, AppError> {
        {
            let state = self.token.read().await;
            if Instant::now() < state.expires_at {
                return Ok(state.access_token.clone());
            }
        }
        self.refresh(http).await
    }

    async fn refresh(&self, http: &reqwest::Client) -> Result<String, AppError> {
        let mut state = self.token.write().await;
        // Check again: another task may have refreshed while we waited for the lock
        if Instant::now() < state.expires_at {
            return Ok(state.access_token.clone());
        }

        eprintln!("[falcon] Token expired, refreshing...");
        let (access_token, expires_at) =
            do_oauth2(http, &self.base_url, &self.client_id, &self.client_secret).await?;

        state.access_token = access_token.clone();
        state.expires_at = expires_at;
        eprintln!("[falcon] Token refreshed");
        Ok(access_token)
    }
}

/// OAuth2 client_credentials; returns (access_token, expiry instant).
async fn do_oauth2(
    http: &reqwest::Client,
    base_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, Instant), AppError> {
    let resp = http
        .post(format!("{base_url}/oauth2/token"))
        .form(&[("client_id", client_id), ("client_secret", client_secret)])
        .timeout(METADATA_TIMEOUT)
        .send()
        .await
        .map_err(|e| AppError::http("OAuth2 request failed", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::api("OAuth2 auth failed", status, body));
    }

    let oauth: OAuthResponse = resp
        .json()
        .await
        .map_err(|e| AppError::http("Failed to parse OAuth2 response", e))?;

    // Refresh 60 seconds before the actual expiry
    let expires_at = Instant::now() + Duration::from_secs(oauth.expires_in.saturating_sub(60));
    Ok((oauth.access_token, expires_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::validate::CLOUDS;

    #[test]
    fn every_supported_cloud_has_its_own_base_url() {
        // `validate_cloud` promises the operator that these regions exist;
        // this is where that promise is actually kept
        let mut seen = std::collections::HashSet::new();
        for cloud in CLOUDS {
            let url = api_base_url(Some(cloud));
            assert!(url.starts_with("https://"), "{cloud} -> {url}");
            assert!(seen.insert(url), "{cloud} shares a base URL with another region");
        }
    }

    #[test]
    fn us_1_is_the_bare_endpoint() {
        assert_eq!(api_base_url(Some("us-1")), "https://api.crowdstrike.com");
    }
}
