use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    Deb,
    Rpm,
    WindowsExe,
}

/// A local sensor file, read into memory at startup.
pub struct Sensor {
    pub filename: String,
    pub data: bytes::Bytes,
    pub sha256: String,
    pub sensor_type: SensorType,
}

/// An entry from the list of available sensors. The field shapes come from
/// the CrowdStrike API, but the type lives here rather than in `falcon`:
/// otherwise the caches and matching would depend on the client, and the
/// client on them.
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
    #[allow(dead_code)]
    pub architectures: Vec<String>,
}
