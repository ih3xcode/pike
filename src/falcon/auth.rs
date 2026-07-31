use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::common::error::AppError;

/// Таймаути на метадані. Завантаження сенсора свідомо без загального
/// таймауту — це сотні мегабайтів, які на повільному каналі йдуть довго;
/// його захищає лише таймаут з'єднання.
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

/// Тримає базовий URL, креденшели й чинний токен. Оновлює його самостійно
/// за 60 секунд до фактичного протермінування, тож виклики ендпоінтів
/// про строк життя токена не думають.
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

    /// Чинний access token; оновлює його, якщо строк вийшов.
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
        // Ще раз перевіряємо: поки чекали на лок, оновити міг хтось інший
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

/// OAuth2 client_credentials; повертає (access_token, момент протермінування).
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

    // Оновлюємось за 60 секунд до фактичного протермінування
    let expires_at = Instant::now() + Duration::from_secs(oauth.expires_in.saturating_sub(60));
    Ok((oauth.access_token, expires_at))
}
