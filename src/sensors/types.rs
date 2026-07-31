use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    Deb,
    Rpm,
    WindowsExe,
}

/// Локальний файл сенсора, прочитаний у памʼять на старті.
pub struct Sensor {
    pub filename: String,
    pub data: bytes::Bytes,
    pub sha256: String,
    pub sensor_type: SensorType,
}

/// Запис зі списку доступних сенсорів. Формат полів диктує CrowdStrike API,
/// але тип живе тут, а не в `falcon`: інакше кеші й зіставлення залежали б
/// від клієнта, а клієнт — від них.
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
