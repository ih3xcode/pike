use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::sensor_match::validate_sensor_filenames;
use crate::types::{Sensor, SensorType};

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

pub fn generate_token() -> String {
    let bytes: [u8; 4] = rand::random();
    hex::encode(bytes)
}

pub fn detect_available_addrs() -> Vec<(String, String)> {
    let mut addrs = Vec::new();
    if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in interfaces {
            if ip.is_ipv4() && !ip.is_loopback() {
                let label = format!("{} ({})", ip, name);
                addrs.push((label, ip.to_string()));
            }
        }
    }
    addrs.sort_by(|a, b| a.1.cmp(&b.1));
    addrs.dedup_by(|a, b| a.1 == b.1);
    addrs.push(("0.0.0.0 (all interfaces)".into(), "0.0.0.0".into()));
    addrs.push(("127.0.0.1 (loopback)".into(), "127.0.0.1".into()));
    addrs
}

pub fn detect_addr() -> String {
    match local_ip_address::local_ip() {
        Ok(ip) => ip.to_string(),
        Err(_) => {
            eprintln!("WARNING: Could not detect local IP, falling back to 127.0.0.1");
            "127.0.0.1".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- detect_sensor_type ---

    #[test]
    fn detect_deb() {
        assert_eq!(detect_sensor_type(&PathBuf::from("falcon_amd64.deb")), Some(SensorType::Deb));
    }

    #[test]
    fn detect_rpm() {
        assert_eq!(detect_sensor_type(&PathBuf::from("falcon.el9.x86_64.rpm")), Some(SensorType::Rpm));
    }

    #[test]
    fn detect_exe() {
        assert_eq!(detect_sensor_type(&PathBuf::from("FalconSensor.exe")), Some(SensorType::WindowsExe));
    }

    #[test]
    fn detect_txt_none() {
        assert_eq!(detect_sensor_type(&PathBuf::from("readme.txt")), None);
    }

    #[test]
    fn detect_no_extension() {
        assert_eq!(detect_sensor_type(&PathBuf::from("Makefile")), None);
    }

    // --- generate_token ---

    #[test]
    fn token_length() {
        let token = generate_token();
        assert_eq!(token.len(), 8);
    }

    #[test]
    fn token_all_hex() {
        let token = generate_token();
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn token_unique() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
    }
}
