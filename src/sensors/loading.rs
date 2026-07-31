use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::common::error::AppError;

use super::matching::validate_sensor_filenames;
use super::types::{Sensor, SensorType};

pub fn detect_sensor_type(path: &Path) -> Option<SensorType> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("deb") => Some(SensorType::Deb),
        Some("rpm") => Some(SensorType::Rpm),
        Some("exe") => Some(SensorType::WindowsExe),
        _ => None,
    }
}

pub fn load_sensors(paths: &[PathBuf]) -> Result<Vec<Sensor>, AppError> {
    let mut sensors = Vec::new();
    for path in paths {
        let sensor_type = detect_sensor_type(path).ok_or_else(|| {
            AppError::Other(format!(
                "Unknown sensor type for '{}'. Supported: .deb, .rpm, .exe",
                path.display()
            ))
        })?;

        let data = std::fs::read(path)
            .map_err(|e| AppError::io(format!("Cannot read '{}'", path.display()), e))?;

        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            hex::encode(hasher.finalize())
        };

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        sensors.push(Sensor {
            filename,
            data: bytes::Bytes::from(data),
            sha256,
            sensor_type,
        });
    }
    for warning in validate_sensor_filenames(&sensors) {
        eprintln!("WARNING: {warning}");
    }

    Ok(sensors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_deb() {
        assert_eq!(
            detect_sensor_type(&PathBuf::from("falcon_amd64.deb")),
            Some(SensorType::Deb)
        );
    }

    #[test]
    fn detect_rpm() {
        assert_eq!(
            detect_sensor_type(&PathBuf::from("falcon.el9.x86_64.rpm")),
            Some(SensorType::Rpm)
        );
    }

    #[test]
    fn detect_exe() {
        assert_eq!(
            detect_sensor_type(&PathBuf::from("FalconSensor.exe")),
            Some(SensorType::WindowsExe)
        );
    }

    #[test]
    fn detect_txt_none() {
        assert_eq!(detect_sensor_type(&PathBuf::from("readme.txt")), None);
    }

    #[test]
    fn detect_no_extension() {
        assert_eq!(detect_sensor_type(&PathBuf::from("Makefile")), None);
    }
}
