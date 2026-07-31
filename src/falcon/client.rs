use serde::Deserialize;

use crate::common::error::AppError;
use crate::sensors::ports::{BoxFuture, SensorDownloader, SensorLister};
use crate::sensors::types::SensorMeta;

use super::auth::{api_base_url, Authenticator, CONNECT_TIMEOUT, METADATA_TIMEOUT};

#[derive(Deserialize)]
struct ResourcesResponse<T> {
    resources: Vec<T>,
}

pub struct FalconClient {
    http: reqwest::Client,
    auth: Authenticator,
}

impl FalconClient {
    pub async fn new(
        client_id: &str,
        client_secret: &str,
        cloud: Option<&str>,
    ) -> Result<Self, AppError> {
        let base_url = api_base_url(cloud).to_string();
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| AppError::http("Cannot build HTTP client", e))?;

        let auth = Authenticator::new(&http, base_url, client_id, client_secret).await?;
        Ok(Self { http, auth })
    }

    pub async fn get_ccid(&self) -> Result<String, AppError> {
        eprintln!("[falcon] Fetching CCID ...");
        let token = self.auth.access_token(&self.http).await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/queries/installers/ccid/v1",
                self.auth.base_url()
            ))
            .bearer_auth(&token)
            .timeout(METADATA_TIMEOUT)
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

        let token = self.auth.access_token(&self.http).await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/combined/installers/v3",
                self.auth.base_url()
            ))
            .bearer_auth(&token)
            .query(&[("filter", &filter)])
            .timeout(METADATA_TIMEOUT)
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

        eprintln!(
            "[falcon] Found {} sensor(s) for platform '{platform}'",
            data.resources.len()
        );
        for s in &data.resources {
            eprintln!(
                "[falcon]   {} ({}, {} bytes)",
                s.name, s.file_type, s.file_size
            );
        }
        Ok(data.resources)
    }

    pub async fn download_sensor(&self, sha256: &str) -> Result<bytes::Bytes, AppError> {
        eprintln!("[falcon] Downloading sensor sha256={} ...", &sha256[..12]);
        let token = self.auth.access_token(&self.http).await?;
        let resp = self
            .http
            .get(format!(
                "{}/sensors/entities/download-installer/v3",
                self.auth.base_url()
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

impl SensorLister for FalconClient {
    fn list<'a>(&'a self, platform: &'a str) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>> {
        Box::pin(self.list_sensors(platform))
    }
}

impl SensorDownloader for FalconClient {
    fn fetch<'a>(&'a self, sha256: &'a str) -> BoxFuture<'a, Result<bytes::Bytes, AppError>> {
        Box::pin(self.download_sensor(sha256))
    }
}
