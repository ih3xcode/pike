use crate::falcon_api::SensorMeta;
use crate::types::{Sensor, SensorType};

/// Map host distro (from /etc/os-release ID) + version to the CrowdStrike
/// distro tag used in RPM filenames (e.g. "el9", "suse15", "amzn2023").
fn distro_tag(distro_id: &str, version: &str) -> Option<String> {
    match distro_id {
        // RHEL family — Fedora uses RHEL-compatible sensors
        "fedora" => {
            let ver: u32 = version.parse().unwrap_or(0);
            // Fedora 41+ targets the same ABI as RHEL 10
            if ver >= 41 {
                Some("el10".into())
            } else {
                Some("el9".into())
            }
        }
        "rhel" | "centos" | "rocky" | "almalinux" | "ol" => {
            let major = version.split('.').next().unwrap_or("9");
            Some(format!("el{major}"))
        }
        // Amazon Linux
        "amzn" => Some(format!("amzn{version}")),
        // SUSE family
        "sles" | "opensuse-leap" | "opensuse" | "opensuse-tumbleweed" => {
            let major = version.split('.').next().unwrap_or("15");
            Some(format!("suse{major}"))
        }
        // PhotonOS
        "photon" => Some(format!("ph{version}")),
        _ => None,
    }
}

/// RHEL-family fallback chain: if exact el version not available, try neighbors.
fn rhel_fallback_tags(tag: &str) -> Vec<String> {
    match tag {
        "el10" => vec!["el10".into(), "el9".into()],
        "el9" => vec!["el9".into(), "el8".into()],
        "el8" => vec!["el8".into(), "el9".into()],
        "el7" => vec!["el7".into(), "el8".into()],
        _ => vec![tag.to_string()],
    }
}

fn is_rhel_family(distro_id: &str) -> bool {
    matches!(
        distro_id,
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol"
    )
}

/// Check if RPM filename contains the given distro tag and host arch.
/// e.g. filename "falcon-sensor-7.33.0-18606.el9.x86_64.rpm" matches tag="el9", arch="x86_64"
fn rpm_filename_matches(filename: &str, tag: &str, host_arch: &str) -> bool {
    filename.contains(&format!(".{tag}.")) && filename.contains(&format!(".{host_arch}.rpm"))
}

/// Check if DEB filename matches host arch.
/// DEB uses amd64/arm64/s390x in filename: falcon-sensor_7.33.0-18606_amd64.deb
fn deb_filename_matches(filename: &str, host_arch: &str) -> bool {
    let deb_arch = match host_arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    filename.contains(&format!("_{deb_arch}.deb"))
}

/// Known RPM distro tags that appear in CrowdStrike sensor filenames.
const KNOWN_RPM_TAGS: &[&str] = &[
    "el7", "el8", "el9", "el10", "amzn2", "amzn2023", "suse11", "suse12", "suse15", "ph3", "ph4",
];

/// Known DEB arch tags in CrowdStrike sensor filenames.
const KNOWN_DEB_ARCHS: &[&str] = &["amd64", "arm64", "s390x"];

/// Known RPM arch tags in CrowdStrike sensor filenames.
const KNOWN_RPM_ARCHS: &[&str] = &["x86_64", "aarch64", "s390x"];

/// Check if an RPM filename contains a recognizable distro tag and architecture.
fn rpm_has_tags(filename: &str) -> bool {
    let has_distro = KNOWN_RPM_TAGS
        .iter()
        .any(|tag| filename.contains(&format!(".{tag}.")));
    let has_arch = KNOWN_RPM_ARCHS
        .iter()
        .any(|arch| filename.contains(&format!(".{arch}.rpm")));
    has_distro && has_arch
}

/// Check if a DEB filename contains a recognizable architecture tag.
fn deb_has_arch(filename: &str) -> bool {
    KNOWN_DEB_ARCHS
        .iter()
        .any(|arch| filename.contains(&format!("_{arch}.deb")))
}

/// Check a single sensor filename for missing tags. Returns a warning message
/// if the filename lacks recognizable distro/arch tags, or `None` if it looks valid.
pub fn check_sensor_filename(filename: &str, sensor_type: SensorType) -> Option<String> {
    match sensor_type {
        SensorType::Rpm => {
            if !rpm_has_tags(filename) {
                Some(format!(
                    "RPM '{}': filename lacks distro/arch tags (expected e.g. '.el9.x86_64.rpm'). \
                     Pike cannot match this sensor to the correct host — expect installation failures \
                     if it is incompatible.",
                    filename
                ))
            } else {
                None
            }
        }
        SensorType::Deb => {
            if !deb_has_arch(filename) {
                Some(format!(
                    "DEB '{}': filename lacks arch tag (expected e.g. '_amd64.deb'). \
                     Pike cannot match this sensor to the correct host — expect installation failures \
                     if it is incompatible.",
                    filename
                ))
            } else {
                None
            }
        }
        SensorType::WindowsExe => None,
    }
}

/// Validate local sensor filenames and print warnings for sensors that lack
/// recognizable tags. Returns a list of warning messages.
pub fn validate_sensor_filenames(sensors: &[Sensor]) -> Vec<String> {
    sensors
        .iter()
        .filter_map(|s| check_sensor_filename(&s.filename, s.sensor_type))
        .collect()
}

/// Find best matching sensor from already-loaded local sensors.
pub fn find_best_local_sensor<'a>(
    sensors: &'a [Sensor],
    target_type: SensorType,
    host_arch: &str,
    distro_id: &str,
    distro_version: &str,
) -> Option<&'a Sensor> {
    let type_matches: Vec<&Sensor> = sensors
        .iter()
        .filter(|s| s.sensor_type == target_type)
        .collect();

    if type_matches.is_empty() {
        return None;
    }

    match target_type {
        SensorType::Deb => {
            // DEB: match by arch in filename
            type_matches
                .iter()
                .find(|s| deb_filename_matches(&s.filename, host_arch))
                .copied()
        }
        SensorType::Rpm => {
            // Тільки точний distro-тег (з ланцюжком для RHEL-родини).
            // Fallback «будь-який RPM цієї архітектури» навмисно відсутній:
            // він віддавав пакети чужих дистрибуцій.
            let tag = distro_tag(distro_id, distro_version)?;
            let tags = if is_rhel_family(distro_id) {
                rhel_fallback_tags(&tag)
            } else {
                vec![tag]
            };
            tags.iter().find_map(|t| {
                type_matches
                    .iter()
                    .find(|s| rpm_filename_matches(&s.filename, t, host_arch))
                    .copied()
            })
        }
        SensorType::WindowsExe => {
            // Windows: single multi-arch binary, just pick first
            type_matches.first().copied()
        }
    }
}

/// Find best matching sensor from API metadata list.
/// API results are sorted by release_date desc, so first match = latest version.
pub fn find_best_api_sensor<'a>(
    metas: &'a [SensorMeta],
    file_type: &str,
    host_arch: &str,
    distro_id: &str,
    distro_version: &str,
) -> Option<&'a SensorMeta> {
    let type_matches: Vec<&SensorMeta> = metas
        .iter()
        .filter(|m| m.file_type == file_type)
        .collect();

    match file_type {
        "deb" => {
            // DEB: match by arch in filename
            type_matches
                .iter()
                .find(|m| deb_filename_matches(&m.name, host_arch))
                .copied()
        }
        "rpm" => {
            // RPM: match by distro tag + arch in filename
            if let Some(tag) = distro_tag(distro_id, distro_version) {
                let tags = if is_rhel_family(distro_id) {
                    rhel_fallback_tags(&tag)
                } else {
                    vec![tag.clone()]
                };
                for t in &tags {
                    if let Some(m) = type_matches
                        .iter()
                        .find(|m| rpm_filename_matches(&m.name, t, host_arch))
                    {
                        eprintln!("[match] RPM matched: tag={t} arch={host_arch} -> {}", m.name);
                        return Some(m);
                    }
                }
                eprintln!(
                    "[match] RPM no match for tags {:?} arch={host_arch}",
                    tags
                );
            } else {
                eprintln!(
                    "[match] RPM unknown distro '{}' — no sensor available",
                    distro_id
                );
            }
            None
        }
        "exe" => {
            // Windows: just pick the latest (first in list)
            type_matches.first().copied()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn test_sensor(filename: &str, sensor_type: SensorType) -> Sensor {
        Sensor {
            filename: filename.to_string(),
            data: Bytes::from_static(b"fake"),
            sha256: "abc123".to_string(),
            sensor_type,
        }
    }

    fn test_meta(name: &str, file_type: &str, os: &str) -> SensorMeta {
        SensorMeta {
            name: name.to_string(),
            sha256: "def456".to_string(),
            platform: "linux".to_string(),
            os: os.to_string(),
            file_type: file_type.to_string(),
            file_size: 1000,
            version: "7.0".to_string(),
            architectures: vec![],
        }
    }

    // --- distro_tag ---

    #[test]
    fn distro_tag_rhel9() {
        assert_eq!(distro_tag("rhel", "9.2"), Some("el9".into()));
    }

    #[test]
    fn distro_tag_rhel8() {
        assert_eq!(distro_tag("rhel", "8.7"), Some("el8".into()));
    }

    #[test]
    fn distro_tag_centos() {
        assert_eq!(distro_tag("centos", "7"), Some("el7".into()));
    }

    #[test]
    fn distro_tag_rocky() {
        assert_eq!(distro_tag("rocky", "9.1"), Some("el9".into()));
    }

    #[test]
    fn distro_tag_alma() {
        assert_eq!(distro_tag("almalinux", "9.3"), Some("el9".into()));
    }

    #[test]
    fn distro_tag_oracle() {
        assert_eq!(distro_tag("ol", "8.6"), Some("el8".into()));
    }

    #[test]
    fn distro_tag_fedora_40() {
        assert_eq!(distro_tag("fedora", "40"), Some("el9".into()));
    }

    #[test]
    fn distro_tag_fedora_41() {
        assert_eq!(distro_tag("fedora", "41"), Some("el10".into()));
    }

    #[test]
    fn distro_tag_amazon_2() {
        assert_eq!(distro_tag("amzn", "2"), Some("amzn2".into()));
    }

    #[test]
    fn distro_tag_amazon_2023() {
        assert_eq!(distro_tag("amzn", "2023"), Some("amzn2023".into()));
    }

    #[test]
    fn distro_tag_suse15() {
        assert_eq!(distro_tag("sles", "15.4"), Some("suse15".into()));
    }

    #[test]
    fn distro_tag_opensuse() {
        assert_eq!(distro_tag("opensuse-leap", "15.5"), Some("suse15".into()));
    }

    #[test]
    fn distro_tag_ubuntu_none() {
        assert_eq!(distro_tag("ubuntu", "22.04"), None);
    }

    #[test]
    fn distro_tag_debian_none() {
        assert_eq!(distro_tag("debian", "12"), None);
    }

    #[test]
    fn distro_tag_unknown() {
        assert_eq!(distro_tag("archlinux", "rolling"), None);
    }

    // --- rhel_fallback_tags ---

    #[test]
    fn fallback_el10() {
        assert_eq!(rhel_fallback_tags("el10"), vec!["el10", "el9"]);
    }

    #[test]
    fn fallback_el9() {
        assert_eq!(rhel_fallback_tags("el9"), vec!["el9", "el8"]);
    }

    #[test]
    fn fallback_el8() {
        assert_eq!(rhel_fallback_tags("el8"), vec!["el8", "el9"]);
    }

    #[test]
    fn fallback_unknown_tag() {
        assert_eq!(rhel_fallback_tags("amzn2023"), vec!["amzn2023"]);
    }

    // --- is_rhel_family ---

    #[test]
    fn rhel_family_true() {
        for id in &["fedora", "rhel", "centos", "rocky", "almalinux", "ol"] {
            assert!(is_rhel_family(id), "{id} should be RHEL family");
        }
    }

    #[test]
    fn rhel_family_false() {
        for id in &["ubuntu", "debian", "sles", "amzn", "archlinux"] {
            assert!(!is_rhel_family(id), "{id} should not be RHEL family");
        }
    }

    // --- rpm_filename_matches ---

    #[test]
    fn rpm_match_el9_x86() {
        assert!(rpm_filename_matches(
            "falcon-sensor-7.33.0-18606.el9.x86_64.rpm",
            "el9",
            "x86_64"
        ));
    }

    #[test]
    fn rpm_mismatch_wrong_distro() {
        assert!(!rpm_filename_matches(
            "falcon-sensor-7.33.0-18606.el9.x86_64.rpm",
            "el8",
            "x86_64"
        ));
    }

    #[test]
    fn rpm_mismatch_wrong_arch() {
        assert!(!rpm_filename_matches(
            "falcon-sensor-7.33.0-18606.el9.x86_64.rpm",
            "el9",
            "aarch64"
        ));
    }

    #[test]
    fn rpm_match_suse_s390x() {
        assert!(rpm_filename_matches(
            "falcon-sensor-7.33.0-18606.suse15.s390x.rpm",
            "suse15",
            "s390x"
        ));
    }

    // --- deb_filename_matches ---

    #[test]
    fn deb_match_amd64() {
        assert!(deb_filename_matches(
            "falcon-sensor_7.33.0-18606_amd64.deb",
            "x86_64"
        ));
    }

    #[test]
    fn deb_match_arm64() {
        assert!(deb_filename_matches(
            "falcon-sensor_7.33.0-18606_arm64.deb",
            "aarch64"
        ));
    }

    #[test]
    fn deb_mismatch_wrong_arch() {
        assert!(!deb_filename_matches(
            "falcon-sensor_7.33.0-18606_amd64.deb",
            "aarch64"
        ));
    }

    #[test]
    fn deb_match_s390x_passthrough() {
        assert!(deb_filename_matches(
            "falcon-sensor_7.33.0-18606_s390x.deb",
            "s390x"
        ));
    }

    // --- find_best_local_sensor ---

    #[test]
    fn local_deb_exact_match() {
        let sensors = vec![
            test_sensor("falcon_amd64.deb", SensorType::Deb),
            test_sensor("falcon_arm64.deb", SensorType::Deb),
        ];
        let result = find_best_local_sensor(&sensors, SensorType::Deb, "aarch64", "ubuntu", "22.04");
        assert_eq!(result.unwrap().filename, "falcon_arm64.deb");
    }

    #[test]
    fn local_deb_no_fallback_wrong_arch() {
        let sensors = vec![test_sensor("falcon_arm64.deb", SensorType::Deb)];
        let result = find_best_local_sensor(&sensors, SensorType::Deb, "x86_64", "ubuntu", "22.04");
        assert!(result.is_none());
    }

    #[test]
    fn local_rpm_exact_match() {
        let sensors = vec![
            test_sensor("falcon.el8.x86_64.rpm", SensorType::Rpm),
            test_sensor("falcon.el9.x86_64.rpm", SensorType::Rpm),
        ];
        let result = find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "rhel", "9.2");
        assert_eq!(result.unwrap().filename, "falcon.el9.x86_64.rpm");
    }

    #[test]
    fn local_rpm_fallback_chain() {
        let sensors = vec![test_sensor("falcon.el8.x86_64.rpm", SensorType::Rpm)];
        let result = find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "rhel", "9");
        assert_eq!(result.unwrap().filename, "falcon.el8.x86_64.rpm");
    }

    #[test]
    fn local_no_match_wrong_type() {
        let sensors = vec![test_sensor("falcon_amd64.deb", SensorType::Deb)];
        let result = find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "rhel", "9");
        assert!(result.is_none());
    }

    #[test]
    fn local_windows_first() {
        let sensors = vec![
            test_sensor("FalconSensor_Windows.exe", SensorType::WindowsExe),
            test_sensor("FalconSensor_Windows_v2.exe", SensorType::WindowsExe),
        ];
        let result = find_best_local_sensor(&sensors, SensorType::WindowsExe, "AMD64", "", "");
        assert_eq!(result.unwrap().filename, "FalconSensor_Windows.exe");
    }

    #[test]
    fn local_empty_sensors() {
        let sensors: Vec<Sensor> = vec![];
        let result = find_best_local_sensor(&sensors, SensorType::Deb, "x86_64", "ubuntu", "22.04");
        assert!(result.is_none());
    }

    // --- find_best_api_sensor ---

    #[test]
    fn api_deb_match() {
        let metas = vec![
            test_meta("falcon_amd64.deb", "deb", "Ubuntu"),
            test_meta("falcon_arm64.deb", "deb", "Ubuntu"),
        ];
        let result = find_best_api_sensor(&metas, "deb", "aarch64", "ubuntu", "22.04");
        assert_eq!(result.unwrap().name, "falcon_arm64.deb");
    }

    #[test]
    fn api_rpm_exact_match() {
        let metas = vec![
            test_meta("falcon.el8.x86_64.rpm", "rpm", "RHEL/CentOS/Oracle 8"),
            test_meta("falcon.el9.x86_64.rpm", "rpm", "RHEL/CentOS/Oracle 9"),
        ];
        let result = find_best_api_sensor(&metas, "rpm", "x86_64", "rhel", "9.2");
        assert_eq!(result.unwrap().name, "falcon.el9.x86_64.rpm");
    }

    #[test]
    fn api_rpm_no_fallback_unknown_distro() {
        let metas = vec![test_meta("falcon.el9.x86_64.rpm", "rpm", "RHEL 9")];
        let result = find_best_api_sensor(&metas, "rpm", "x86_64", "archlinux", "rolling");
        assert!(result.is_none());
    }

    #[test]
    fn api_exe_first() {
        let metas = vec![test_meta("FalconSensor_Windows.exe", "exe", "Windows")];
        let result = find_best_api_sensor(&metas, "exe", "AMD64", "", "");
        assert_eq!(result.unwrap().name, "FalconSensor_Windows.exe");
    }

    #[test]
    fn api_rpm_rhel_fallback_chain() {
        // el9 not available, should fall back to el8 via RHEL chain
        let metas = vec![test_meta("falcon.el8.x86_64.rpm", "rpm", "RHEL 8")];
        let result = find_best_api_sensor(&metas, "rpm", "x86_64", "rhel", "9");
        assert_eq!(result.unwrap().name, "falcon.el8.x86_64.rpm");
    }

    #[test]
    fn api_rpm_no_fallback_wrong_arch() {
        let metas = vec![test_meta("falcon.el9.x86_64.rpm", "rpm", "RHEL 9")];
        let result = find_best_api_sensor(&metas, "rpm", "aarch64", "rhel", "9");
        assert!(result.is_none());
    }

    #[test]
    fn api_deb_no_fallback_wrong_arch() {
        let metas = vec![test_meta("falcon_amd64.deb", "deb", "Ubuntu")];
        let result = find_best_api_sensor(&metas, "deb", "aarch64", "ubuntu", "22.04");
        assert!(result.is_none());
    }

    #[test]
    fn api_no_match_unknown_type() {
        let metas = vec![test_meta("falcon_amd64.deb", "deb", "Ubuntu")];
        let result = find_best_api_sensor(&metas, "msi", "x86_64", "", "");
        assert!(result.is_none());
    }

    // --- validate_sensor_filenames ---

    #[test]
    fn validate_good_rpm() {
        let sensors = vec![test_sensor("falcon.el9.x86_64.rpm", SensorType::Rpm)];
        assert!(validate_sensor_filenames(&sensors).is_empty());
    }

    #[test]
    fn validate_bad_rpm_no_tags() {
        let sensors = vec![test_sensor("sensor1.rpm", SensorType::Rpm)];
        let warnings = validate_sensor_filenames(&sensors);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("sensor1.rpm"));
    }

    #[test]
    fn validate_bad_rpm_missing_arch() {
        let sensors = vec![test_sensor("falcon.el9.rpm", SensorType::Rpm)];
        let warnings = validate_sensor_filenames(&sensors);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn validate_good_deb() {
        let sensors = vec![test_sensor("falcon_amd64.deb", SensorType::Deb)];
        assert!(validate_sensor_filenames(&sensors).is_empty());
    }

    #[test]
    fn validate_bad_deb_no_arch() {
        let sensors = vec![test_sensor("sensor.deb", SensorType::Deb)];
        let warnings = validate_sensor_filenames(&sensors);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("sensor.deb"));
    }

    #[test]
    fn validate_exe_no_warning() {
        let sensors = vec![test_sensor("anything.exe", SensorType::WindowsExe)];
        assert!(validate_sensor_filenames(&sensors).is_empty());
    }

    #[test]
    fn validate_mixed_good_and_bad() {
        let sensors = vec![
            test_sensor("falcon.el9.x86_64.rpm", SensorType::Rpm),
            test_sensor("mystery.rpm", SensorType::Rpm),
            test_sensor("falcon_amd64.deb", SensorType::Deb),
            test_sensor("blob.deb", SensorType::Deb),
        ];
        let warnings = validate_sensor_filenames(&sensors);
        assert_eq!(warnings.len(), 2);
    }

    // --- no fallback to first sensor ---

    #[test]
    fn local_rpm_no_fallback_untagged() {
        let sensors = vec![
            test_sensor("sensor1.rpm", SensorType::Rpm),
            test_sensor("sensor2.rpm", SensorType::Rpm),
        ];
        let result = find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "rhel", "9");
        assert!(result.is_none());
    }

    #[test]
    fn local_rpm_no_cross_distro_fallback() {
        // Кеш накопичив SUSE-сенсор; RHEL-хост не має його отримати
        let sensors = vec![test_sensor(
            "falcon-sensor-7.33.0-18606.suse15.x86_64.rpm",
            SensorType::Rpm,
        )];
        let result = find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "rhel", "10");
        assert!(result.is_none());
    }

    #[test]
    fn local_rpm_no_fallback_unknown_distro() {
        let sensors = vec![test_sensor("falcon.el9.x86_64.rpm", SensorType::Rpm)];
        let result =
            find_best_local_sensor(&sensors, SensorType::Rpm, "x86_64", "archlinux", "rolling");
        assert!(result.is_none());
    }
}
