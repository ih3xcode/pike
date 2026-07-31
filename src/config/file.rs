use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::common::AppError;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub falcon: FalconSection,
    #[serde(default)]
    pub sensors: SensorsSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSection {
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub addr: Option<String>,
    pub public_url: Option<String>,
    pub token: Option<String>,
    pub timeout_minutes: Option<u64>,
    pub max_downloads: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FalconSection {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cloud: Option<String>,
    pub cid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorsSection {
    pub cache_dir: Option<PathBuf>,
    pub metadata_ttl_minutes: Option<u64>,
    pub cache_max_bytes: Option<u64>,
    pub tags: Option<String>,
    pub default_tag: Option<bool>,
    pub files: Option<Vec<PathBuf>>,
}

pub fn load_file(path: &Path) -> Result<FileConfig, AppError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AppError::io(format!("Cannot read config '{}'", path.display()), e))?;
    toml::from_str(&text)
        .map_err(|e| AppError::Other(format!("Invalid config '{}': {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_is_an_error() {
        let err = toml::from_str::<FileConfig>("[server]\nnonsense = 1\n");
        assert!(err.is_err());
    }
}
