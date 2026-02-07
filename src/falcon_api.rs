use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::AppError;

pub struct FalconClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    token: RwLock<TokenState>,
}

struct TokenState {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SensorMeta {
    pub name: String,
    pub sha256: String,
    #[allow(dead_code)]
    pub platform: String,
    pub os: String,
    pub file_type: String,
    pub file_size: u64,
    #[allow(dead_code)]
    pub version: String,
    #[serde(default)]
    pub architectures: Vec<String>,
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

#[derive(Deserialize)]
struct ResourcesResponse<T> {
    resources: Vec<T>,
}

fn api_base_url(cloud: Option<&str>) -> &str {
    match cloud {
        Some("eu-1") => "https://api.eu-1.crowdstrike.com",
        Some("us-2") => "https://api.us-2.crowdstrike.com",
        Some("us-gov-1") => "https://api.laggar.gcw.crowdstrike.com",
        Some("us-gov-2") => "https://api.us-gov-2.crowdstrike.com",
        _ => "https://api.crowdstrike.com",
    }
}

/// Perform OAuth2 client_credentials flow, return (access_token, expires_at).
async fn do_oauth2(
    http: &reqwest::Client,
    base_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, Instant), AppError> {
    let resp = http
        .post(format!("{base_url}/oauth2/token"))
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
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

    // Refresh 60s before actual expiry
    let expires_at = Instant::now() + Duration::from_secs(oauth.expires_in.saturating_sub(60));
    Ok((oauth.access_token, expires_at))
}

impl FalconClient {
    pub async fn new(
        client_id: &str,
        client_secret: &str,
        cloud: Option<&str>,
    ) -> Result<Self, AppError> {
        let base_url = api_base_url(cloud).to_string();
        let http = reqwest::Client::new();

        eprintln!("[falcon] Authenticating to {base_url} ...");
        let (access_token, expires_at) =
            do_oauth2(&http, &base_url, client_id, client_secret).await?;

        eprintln!("[falcon] Authenticated successfully");
        Ok(Self {
            http,
            base_url,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            token: RwLock::new(TokenState {
                access_token,
                expires_at,
            }),
        })
    }

    /// Return a valid access token, refreshing automatically if expired.
    async fn access_token(&self) -> Result<String, AppError> {
        {
            let state = self.token.read().await;
            if Instant::now() < state.expires_at {
                return Ok(state.access_token.clone());
            }
        }
        self.refresh_token().await
    }

    async fn refresh_token(&self) -> Result<String, AppError> {
        let mut state = self.token.write().await;
        // Double-check: another task may have refreshed while we waited for the lock
        if Instant::now() < state.expires_at {
            return Ok(state.access_token.clone());
        }

        eprintln!("[falcon] Token expired, refreshing...");
        let (access_token, expires_at) =
            do_oauth2(&self.http, &self.base_url, &self.client_id, &self.client_secret).await?;

        state.access_token = access_token.clone();
        state.expires_at = expires_at;
        eprintln!("[falcon] Token refreshed");
        Ok(access_token)
    }

    pub async fn get_ccid(&self) -> Result<String, AppError> {
        eprintln!("[falcon] Fetching CCID ...");
        let token = self.access_token().await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/queries/installers/ccid/v1",
                self.base_url
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| AppError::http("CCID request failed", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::api("CCID fetch failed", status, body));
        }

        let data: ResourcesResponse<String> = resp
            .json()
            .await
            .map_err(|e| AppError::http("Failed to parse CCID response", e))?;

        let ccid = data
            .resources
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Other("No CCID returned from API".into()))?;

        eprintln!("[falcon] CCID: {ccid}");
        Ok(ccid)
    }

    pub async fn list_sensors(&self, platform: &str) -> Result<Vec<SensorMeta>, AppError> {
        let filter = format!("platform:'{platform}'");
        eprintln!("[falcon] Listing sensors: filter={filter}");

        let token = self.access_token().await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/combined/installers/v3",
                self.base_url
            ))
            .bearer_auth(&token)
            .query(&[("filter", &filter)])
            .send()
            .await
            .map_err(|e| AppError::http("List sensors request failed", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::api("List sensors failed", status, body));
        }

        let data: ResourcesResponse<SensorMeta> = resp
            .json()
            .await
            .map_err(|e| AppError::http("Failed to parse sensor list", e))?;

        eprintln!("[falcon] Found {} sensor(s) for platform '{platform}'", data.resources.len());
        for s in &data.resources {
            eprintln!("[falcon]   {} ({}, {} bytes)", s.name, s.file_type, s.file_size);
        }
        Ok(data.resources)
    }

    pub async fn download_sensor(&self, sha256: &str) -> Result<bytes::Bytes, AppError> {
        eprintln!("[falcon] Downloading sensor sha256={} ...", &sha256[..12]);
        let token = self.access_token().await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/entities/download-installer/v3",
                self.base_url
            ))
            .bearer_auth(&token)
            .query(&[("id", sha256)])
            .send()
            .await
            .map_err(|e| AppError::http("Sensor download request failed", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::api("Sensor download failed", status, body));
        }

        let data = resp
            .bytes()
            .await
            .map_err(|e| AppError::http("Failed to read sensor data", e))?;

        eprintln!("[falcon] Downloaded {} bytes", data.len());
        Ok(data)
    }
}
