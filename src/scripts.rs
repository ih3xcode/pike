const LINUX_TEMPLATE: &str = include_str!("../templates/linux.sh");
const WINDOWS_TEMPLATE: &str = include_str!("../templates/windows.ps1");

pub const DEFAULT_TAG: &str = "deployment/pike";

pub fn resolve_tags(user_tags: Option<&str>, include_default: bool) -> Option<String> {
    let user = user_tags.map(|t| t.trim()).filter(|t| !t.is_empty());
    match (include_default, user) {
        (true, Some(t)) => Some(format!("{DEFAULT_TAG},{t}")),
        (true, None) => Some(DEFAULT_TAG.to_string()),
        (false, Some(t)) => Some(t.to_string()),
        (false, None) => None,
    }
}

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

fn sanitize_tags(tags: &str) -> Option<String> {
    let s: String = tags
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ',' | '-' | '_' | '/' | '.' | '@'))
        .collect();
    if s.is_empty() || s.len() > 256 {
        None
    } else {
        Some(s)
    }
}

pub fn generate_linux_script(base_url: &str, cid: &str, cloud: Option<&str>, tags: Option<&str>) -> String {
    let safe_cid = sanitize_cid(cid);
    let cloud_flag = cloud
        .and_then(sanitize_cloud)
        .map(|c| format!("--cloud={c}"))
        .unwrap_or_default();
    let tags_flag = tags
        .and_then(sanitize_tags)
        .map(|t| format!("--tags='{t}'"))
        .unwrap_or_default();

    LINUX_TEMPLATE
        .replace("{{BASE_URL}}", base_url)
        .replace("{{CID}}", &safe_cid)
        .replace("{{CLOUD_FLAG}}", &cloud_flag)
        .replace("{{TAGS_FLAG}}", &tags_flag)
}

pub fn generate_windows_script(base_url: &str, cid: &str, cloud: Option<&str>, tags: Option<&str>) -> String {
    let safe_cid = sanitize_cid(cid);
    let cloud_param = cloud
        .and_then(sanitize_cloud)
        .map(|c| format!("CLOUD={c}"))
        .unwrap_or_default();
    let tags_param = tags
        .and_then(sanitize_tags)
        .map(|t| format!("GROUPING_TAGS={t}"))
        .unwrap_or_default();

    WINDOWS_TEMPLATE
        .replace("{{BASE_URL}}", base_url)
        .replace("{{CID}}", &safe_cid)
        .replace("{{CLOUD_PARAM}}", &cloud_param)
        .replace("{{TAGS_PARAM}}", &tags_param)
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

    // --- resolve_tags ---

    #[test]
    fn resolve_tags_default_only() {
        assert_eq!(resolve_tags(None, true), Some("deployment/pike".into()));
    }

    #[test]
    fn resolve_tags_default_plus_user() {
        assert_eq!(
            resolve_tags(Some("env/prod,web"), true),
            Some("deployment/pike,env/prod,web".into())
        );
    }

    #[test]
    fn resolve_tags_no_default_with_user() {
        assert_eq!(
            resolve_tags(Some("env/prod"), false),
            Some("env/prod".into())
        );
    }

    #[test]
    fn resolve_tags_no_default_no_user() {
        assert_eq!(resolve_tags(None, false), None);
    }

    #[test]
    fn resolve_tags_default_with_empty_user() {
        assert_eq!(resolve_tags(Some("  "), true), Some("deployment/pike".into()));
    }

    #[test]
    fn resolve_tags_no_default_with_empty_user() {
        assert_eq!(resolve_tags(Some("  "), false), None);
    }

    // --- sanitize_tags ---

    #[test]
    fn sanitize_tags_normal() {
        assert_eq!(sanitize_tags("Tag1,Tag2"), Some("Tag1,Tag2".into()));
    }

    #[test]
    fn sanitize_tags_allowed_chars() {
        assert_eq!(
            sanitize_tags("env/prod,role-web,v1.0,team@ops"),
            Some("env/prod,role-web,v1.0,team@ops".into())
        );
    }

    #[test]
    fn sanitize_tags_strips_bad_chars() {
        assert_eq!(sanitize_tags("Tag1;rm -rf /"), Some("Tag1rm-rf/".into()));
    }

    #[test]
    fn sanitize_tags_empty() {
        assert_eq!(sanitize_tags(""), None);
    }

    #[test]
    fn sanitize_tags_all_bad_chars() {
        assert_eq!(sanitize_tags("!#$%^&*()"), None);
    }

    #[test]
    fn sanitize_tags_too_long() {
        let long = "a".repeat(257);
        assert_eq!(sanitize_tags(&long), None);
    }

    #[test]
    fn sanitize_tags_exactly_256() {
        let exact = "a".repeat(256);
        assert_eq!(sanitize_tags(&exact), Some(exact));
    }

    // --- generate_linux_script ---

    #[test]
    fn linux_script_contains_cid() {
        let script = generate_linux_script("http://10.0.0.1:8080/tok", "AABBCC", None, None);
        assert!(script.contains("AABBCC"));
    }

    #[test]
    fn linux_script_contains_base_url() {
        let script = generate_linux_script("http://10.0.0.1:8080/tok", "AABB", None, None);
        assert!(script.contains("http://10.0.0.1:8080/tok"));
    }

    #[test]
    fn linux_script_cloud_flag() {
        let script = generate_linux_script("http://host", "AA", Some("us-1"), None);
        assert!(script.contains("--cloud=us-1"));
    }

    #[test]
    fn linux_script_no_cloud_flag() {
        let script = generate_linux_script("http://host", "AA", None, None);
        assert!(!script.contains("--cloud="));
    }

    #[test]
    fn linux_script_with_tags() {
        let script = generate_linux_script("http://host", "AA", None, Some("Tag1,Tag2"));
        assert!(script.contains("--tags='Tag1,Tag2'"));
    }

    #[test]
    fn linux_script_no_tags() {
        let script = generate_linux_script("http://host", "AA", None, None);
        assert!(!script.contains("--tags="));
    }

    // --- generate_windows_script ---

    #[test]
    fn windows_script_contains_cid() {
        let script = generate_windows_script("http://host/tok", "DDEEFF", None, None);
        assert!(script.contains("DDEEFF"));
    }

    #[test]
    fn windows_script_contains_base_url() {
        let script = generate_windows_script("http://host/tok", "AA", None, None);
        assert!(script.contains("http://host/tok"));
    }

    #[test]
    fn windows_script_cloud_param() {
        let script = generate_windows_script("http://host", "AA", Some("eu-1"), None);
        assert!(script.contains("CLOUD=eu-1"));
    }

    #[test]
    fn windows_script_no_cloud_param() {
        let script = generate_windows_script("http://host", "AA", None, None);
        assert!(!script.contains("CLOUD="));
    }

    #[test]
    fn windows_script_with_tags() {
        let script = generate_windows_script("http://host", "AA", None, Some("Tag1,Tag2"));
        assert!(script.contains("GROUPING_TAGS=Tag1,Tag2"));
    }

    #[test]
    fn windows_script_no_tags() {
        let script = generate_windows_script("http://host", "AA", None, None);
        assert!(!script.contains("GROUPING_TAGS="));
    }
}
