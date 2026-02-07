const LINUX_TEMPLATE: &str = include_str!("../templates/linux.sh");
const WINDOWS_TEMPLATE: &str = include_str!("../templates/windows.ps1");

fn sanitize_cid(cid: &str) -> String {
    cid.chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect()
}

fn sanitize_cloud(cloud: &str) -> Option<&str> {
    match cloud {
        "us-1" | "us-2" | "eu-1" | "us-gov-1" | "us-gov-2" => Some(cloud),
        _ => None,
    }
}

pub fn generate_linux_script(base_url: &str, cid: &str, cloud: Option<&str>) -> String {
    let safe_cid = sanitize_cid(cid);
    let cloud_flag = cloud
        .and_then(sanitize_cloud)
        .map(|c| format!("--cloud={c}"))
        .unwrap_or_default();

    LINUX_TEMPLATE
        .replace("{{BASE_URL}}", base_url)
        .replace("{{CID}}", &safe_cid)
        .replace("{{CLOUD_FLAG}}", &cloud_flag)
}

pub fn generate_windows_script(base_url: &str, cid: &str, cloud: Option<&str>) -> String {
    let safe_cid = sanitize_cid(cid);
    let cloud_param = cloud
        .and_then(sanitize_cloud)
        .map(|c| format!("CLOUD={c}"))
        .unwrap_or_default();

    WINDOWS_TEMPLATE
        .replace("{{BASE_URL}}", base_url)
        .replace("{{CID}}", &safe_cid)
        .replace("{{CLOUD_PARAM}}", &cloud_param)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_cid ---

    #[test]
    fn sanitize_cid_normal_hex() {
        assert_eq!(sanitize_cid("ABCDEF01234567890abcdef-12"), "ABCDEF01234567890abcdef-12");
    }

    #[test]
    fn sanitize_cid_injection() {
        // Only hex chars and '-' survive; ";" space "/" are stripped
        assert_eq!(sanitize_cid("\"; rm -rf /"), "-f");
    }

    #[test]
    fn sanitize_cid_empty() {
        assert_eq!(sanitize_cid(""), "");
    }

    #[test]
    fn sanitize_cid_mixed() {
        assert_eq!(sanitize_cid("abc123!@#$%^&*()def"), "abc123def");
    }

    // --- sanitize_cloud ---

    #[test]
    fn sanitize_cloud_valid() {
        for cloud in &["us-1", "us-2", "eu-1", "us-gov-1", "us-gov-2"] {
            assert_eq!(sanitize_cloud(cloud), Some(*cloud));
        }
    }

    #[test]
    fn sanitize_cloud_invalid() {
        assert_eq!(sanitize_cloud("us-3"), None);
        assert_eq!(sanitize_cloud("ap-1"), None);
        assert_eq!(sanitize_cloud("'; DROP TABLE"), None);
    }

    #[test]
    fn sanitize_cloud_empty() {
        assert_eq!(sanitize_cloud(""), None);
    }

    // --- generate_linux_script ---

    #[test]
    fn linux_script_contains_cid() {
        let script = generate_linux_script("http://10.0.0.1:8080/tok", "AABBCC", None);
        assert!(script.contains("AABBCC"));
    }

    #[test]
    fn linux_script_contains_base_url() {
        let script = generate_linux_script("http://10.0.0.1:8080/tok", "AABB", None);
        assert!(script.contains("http://10.0.0.1:8080/tok"));
    }

    #[test]
    fn linux_script_cloud_flag() {
        let script = generate_linux_script("http://host", "AA", Some("us-1"));
        assert!(script.contains("--cloud=us-1"));
    }

    #[test]
    fn linux_script_no_cloud_flag() {
        let script = generate_linux_script("http://host", "AA", None);
        assert!(!script.contains("--cloud="));
    }

    // --- generate_windows_script ---

    #[test]
    fn windows_script_contains_cid() {
        let script = generate_windows_script("http://host/tok", "DDEEFF", None);
        assert!(script.contains("DDEEFF"));
    }

    #[test]
    fn windows_script_contains_base_url() {
        let script = generate_windows_script("http://host/tok", "AA", None);
        assert!(script.contains("http://host/tok"));
    }

    #[test]
    fn windows_script_cloud_param() {
        let script = generate_windows_script("http://host", "AA", Some("eu-1"));
        assert!(script.contains("CLOUD=eu-1"));
    }

    #[test]
    fn windows_script_no_cloud_param() {
        let script = generate_windows_script("http://host", "AA", None);
        assert!(!script.contains("CLOUD="));
    }
}
