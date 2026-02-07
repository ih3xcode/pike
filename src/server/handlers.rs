use axum::{
    extract::{OriginalUri, Path, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::scripts;
use crate::sensor_match::{find_best_api_sensor, find_best_local_sensor};
use crate::types::*;

use super::helpers::{host_header, log_request, peer_addr, resolve_base_url};

// --- Route handlers (5 total, mounted via Router::nest for token prefix) ---

pub async fn install_sh(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_install_sh(&state, &req, &remote, &path)
}

pub async fn install_ps1(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_install_ps1(&state, &req, &remote, &path)
}

pub async fn sensor_download(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(filename): Path<String>,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_sensor(&state, &filename, &remote, &path).await
}

pub async fn callback(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_callback(&state, req, &remote, &path).await
}

pub async fn done(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_done(&state, req, &remote, &path).await
}

// --- Shared logic ---

fn serve_install_sh(
    state: &AppState,
    req: &axum::extract::Request,
    remote: &str,
    path: &str,
) -> Response {
    let host = host_header(req);

    if state.cid.is_empty() {
        log_request("GET", path, remote, 404, "no CID configured");
        return StatusCode::NOT_FOUND.into_response();
    }

    let base_url = resolve_base_url(state, &host);
    let script = scripts::generate_linux_script(&base_url, &state.cid, state.cloud.as_deref());

    log_request("GET", path, remote, 200, "linux script");
    eprintln!("[script] Served linux install script to {remote} (base_url={base_url})");

    let mut resp = script.into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}

fn serve_install_ps1(
    state: &AppState,
    req: &axum::extract::Request,
    remote: &str,
    path: &str,
) -> Response {
    let host = host_header(req);

    if state.cid.is_empty() {
        log_request("GET", path, remote, 404, "no CID configured");
        return StatusCode::NOT_FOUND.into_response();
    }

    let base_url = resolve_base_url(state, &host);
    let script = scripts::generate_windows_script(&base_url, &state.cid, state.cloud.as_deref());

    log_request("GET", path, remote, 200, "windows script");
    eprintln!("[script] Served windows install script to {remote} (base_url={base_url})");

    let mut resp = script.into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect()
}

async fn serve_sensor(
    state: &AppState,
    filename: &str,
    remote: &str,
    path: &str,
) -> Response {
    let sensors = state.sensors.read().await;
    let Some(sensor) = sensors.iter().find(|s| s.filename == filename) else {
        log_request("GET", path, remote, 404, "sensor not found");
        return StatusCode::NOT_FOUND.into_response();
    };

    let count = state
        .download_count
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;

    let extra = if state.max_downloads > 0 {
        format!("download {}/{}", count, state.max_downloads)
    } else {
        format!("download {}", count)
    };
    log_request("GET", path, remote, 200, &extra);

    eprintln!("[sensor] Serving {} to {remote} ({} bytes)", sensor.filename, sensor.data.len());

    if state.max_downloads > 0 && count >= state.max_downloads {
        eprintln!("[server] Download limit reached ({count}/{}), shutting down", state.max_downloads);
        state.shutdown_notify.notify_one();
    }

    // Bytes::clone() is O(1) — no full data copy
    let mut resp = sensor.data.clone().into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    let safe_name = sanitize_filename(&sensor.filename);
    resp.headers_mut().insert(
        "content-disposition",
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", safe_name))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    resp
}

// --- Callback parsing ---

struct CallbackInfo {
    hostname: String,
    pkg_type: String,
    arch: String,
    distro_id: String,
    distro_version: String,
    target_type: SensorType,
}

fn is_valid_hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn is_valid_arch(s: &str) -> bool {
    matches!(
        s,
        "x86_64" | "amd64" | "AMD64" | "aarch64" | "arm64" | "s390x" | "ppc64le"
    )
}

fn is_valid_distro_field(s: &str) -> bool {
    s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn parse_callback(body: &str) -> Result<CallbackInfo, &'static str> {
    let parts: Vec<&str> = body.trim().splitn(5, '|').collect();
    if parts.len() < 3 {
        return Err("invalid callback format");
    }

    let hostname = parts[0];
    if !is_valid_hostname(hostname) {
        return Err("invalid hostname");
    }

    let arch = parts[2];
    if !is_valid_arch(arch) {
        return Err("unsupported architecture");
    }

    let pkg_type = parts[1];
    let target_type = match pkg_type {
        "deb" => SensorType::Deb,
        "rpm" => SensorType::Rpm,
        "exe" => SensorType::WindowsExe,
        _ => return Err("unsupported package type"),
    };

    let distro_id = if parts.len() > 3 { parts[3] } else { "" };
    let distro_version = if parts.len() > 4 { parts[4] } else { "" };

    if !is_valid_distro_field(distro_id) || !is_valid_distro_field(distro_version) {
        return Err("invalid distro field");
    }

    Ok(CallbackInfo {
        hostname: hostname.to_string(),
        pkg_type: pkg_type.to_string(),
        arch: arch.to_string(),
        distro_id: distro_id.to_string(),
        distro_version: distro_version.to_string(),
        target_type,
    })
}

/// Try to find a sensor via API: list available sensors, match, download, and cache.
async fn try_api_sensor(
    state: &AppState,
    info: &CallbackInfo,
) -> Option<(String, String)> {
    let client = state.falcon_client.as_ref()?;

    eprintln!("[host] {}: no local match, querying API ...", info.hostname);
    let platform_str = match info.target_type {
        SensorType::Deb | SensorType::Rpm => "linux",
        SensorType::WindowsExe => "windows",
    };

    let metas = match client.list_sensors(platform_str).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[host] {}: API list sensors failed — {}", info.hostname, e);
            return None;
        }
    };

    let file_type_str = match info.target_type {
        SensorType::Deb => "deb",
        SensorType::Rpm => "rpm",
        SensorType::WindowsExe => "exe",
    };

    let meta = find_best_api_sensor(&metas, file_type_str, &info.arch, &info.distro_id, &info.distro_version)?;

    eprintln!("[host] {}: best match — {} (os={}, arch={:?})", info.hostname, meta.name, meta.os, meta.architectures);
    eprintln!("[host] {}: downloading {} from API ...", info.hostname, meta.name);

    let data = match client.download_sensor(&meta.sha256).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[host] {}: API download failed — {}", info.hostname, e);
            return None;
        }
    };

    let sensor_filename = meta.name.clone();
    let sensor_sha256 = meta.sha256.clone();
    let sensor_type = info.target_type;

    // Check for race condition: another request may have cached the same sensor
    {
        let mut sensors = state.sensors.write().await;
        if !sensors.iter().any(|s| s.sha256 == sensor_sha256) {
            sensors.push(Sensor {
                filename: sensor_filename.clone(),
                data,
                sha256: sensor_sha256.clone(),
                sensor_type,
            });
        }
    }

    eprintln!("[host] {}: sensor ready — {sensor_filename}", info.hostname);
    Some((sensor_filename, sensor_sha256))
}

async fn serve_callback(
    state: &AppState,
    req: axum::extract::Request,
    remote: &str,
    path: &str,
) -> Response {
    let body = match axum::body::to_bytes(req.into_body(), 4096).await {
        Ok(b) => String::from_utf8_lossy(&b).to_string(),
        Err(_) => {
            log_request("POST", path, remote, 400, "bad body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    let info = match parse_callback(&body) {
        Ok(i) => i,
        Err(reason) => {
            log_request("POST", path, remote, 400, reason);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    // Record host as Registered
    state.push_host(HostEntry {
        hostname: info.hostname.clone(),
        platform: info.pkg_type.clone(),
        arch: info.arch.clone(),
        ip: remote.to_string(),
        status: HostStatus::Registered,
        time: chrono::Local::now(),
    });

    let distro_info = if info.distro_id.is_empty() {
        String::new()
    } else {
        format!(" distro={}/{}", info.distro_id, info.distro_version)
    };

    log_request(
        "POST",
        path,
        remote,
        200,
        &format!("cb: {} {}/{}{distro_info}", info.hostname, info.pkg_type, info.arch),
    );
    eprintln!("[host] {} registered (ip={remote}, type={}, arch={}{distro_info})", info.hostname, info.pkg_type, info.arch);

    // Check existing sensors (smart match by filename pattern)
    {
        let sensors = state.sensors.read().await;
        eprintln!("[host] {}: looking for {}/{} sensor in {} loaded sensor(s)", info.hostname, info.pkg_type, info.arch, sensors.len());
        let matched = find_best_local_sensor(&sensors, info.target_type, &info.arch, &info.distro_id, &info.distro_version);
        if let Some(sensor) = matched {
            eprintln!("[host] {}: matched local sensor {}", info.hostname, sensor.filename);
            state.update_host_status(&info.hostname, HostStatus::SensorReady);
            let response = format!("{}|{}", sensor.filename, sensor.sha256);
            return response.into_response();
        }
    }

    // Try to download from API if available
    if let Some((filename, sha256)) = try_api_sensor(state, &info).await {
        state.update_host_status(&info.hostname, HostStatus::SensorReady);
        return format!("{filename}|{sha256}").into_response();
    }

    if state.falcon_client.is_none() {
        eprintln!("[host] {}: no local sensor and no API client configured", info.hostname);
    } else {
        eprintln!("[host] {}: no matching sensor found via API", info.hostname);
    }

    eprintln!("[host] {} FAILED: no matching sensor available", info.hostname);
    state.update_host_status(
        &info.hostname,
        HostStatus::Failed("no matching sensor".into()),
    );
    (StatusCode::NOT_FOUND, "no matching sensor available").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_valid_hostname ---

    #[test]
    fn hostname_valid() {
        assert!(is_valid_hostname("web-server.example.com"));
        assert!(is_valid_hostname("host_01"));
        assert!(is_valid_hostname("a"));
    }

    #[test]
    fn hostname_empty() {
        assert!(!is_valid_hostname(""));
    }

    #[test]
    fn hostname_too_long() {
        let long = "a".repeat(254);
        assert!(!is_valid_hostname(&long));
    }

    #[test]
    fn hostname_253_ok() {
        let exact = "a".repeat(253);
        assert!(is_valid_hostname(&exact));
    }

    #[test]
    fn hostname_special_chars() {
        assert!(!is_valid_hostname("host;rm -rf /"));
        assert!(!is_valid_hostname("host name"));
        assert!(!is_valid_hostname("host\nname"));
    }

    #[test]
    fn hostname_unicode() {
        assert!(!is_valid_hostname("höst"));
    }

    // --- is_valid_arch ---

    #[test]
    fn arch_valid_all() {
        for arch in &["x86_64", "amd64", "AMD64", "aarch64", "arm64", "s390x", "ppc64le"] {
            assert!(is_valid_arch(arch), "{arch} should be valid");
        }
    }

    #[test]
    fn arch_invalid() {
        assert!(!is_valid_arch("i386"));
        assert!(!is_valid_arch("i686"));
        assert!(!is_valid_arch(""));
        assert!(!is_valid_arch("x86"));
    }

    // --- is_valid_distro_field ---

    #[test]
    fn distro_field_valid() {
        assert!(is_valid_distro_field("ubuntu"));
        assert!(is_valid_distro_field("22.04"));
        assert!(is_valid_distro_field("opensuse-leap"));
        assert!(is_valid_distro_field("")); // empty is valid (optional field)
    }

    #[test]
    fn distro_field_too_long() {
        let long = "a".repeat(65);
        assert!(!is_valid_distro_field(&long));
    }

    #[test]
    fn distro_field_special_chars() {
        assert!(!is_valid_distro_field("ubuntu; rm -rf /"));
        assert!(!is_valid_distro_field("distro\nid"));
    }

    // --- parse_callback ---

    #[test]
    fn parse_callback_valid_5_fields() {
        let result = parse_callback("myhost|deb|x86_64|ubuntu|22.04").unwrap();
        assert_eq!(result.hostname, "myhost");
        assert_eq!(result.pkg_type, "deb");
        assert_eq!(result.arch, "x86_64");
        assert_eq!(result.distro_id, "ubuntu");
        assert_eq!(result.distro_version, "22.04");
        assert_eq!(result.target_type, SensorType::Deb);
    }

    #[test]
    fn parse_callback_valid_3_fields() {
        let result = parse_callback("winhost|exe|AMD64").unwrap();
        assert_eq!(result.hostname, "winhost");
        assert_eq!(result.pkg_type, "exe");
        assert_eq!(result.arch, "AMD64");
        assert_eq!(result.distro_id, "");
        assert_eq!(result.distro_version, "");
        assert_eq!(result.target_type, SensorType::WindowsExe);
    }

    #[test]
    fn parse_callback_rpm() {
        let result = parse_callback("rhelbox|rpm|aarch64|rhel|9.2").unwrap();
        assert_eq!(result.target_type, SensorType::Rpm);
    }

    #[test]
    fn parse_callback_too_few_fields() {
        assert!(parse_callback("host|deb").is_err());
        assert!(parse_callback("host").is_err());
        assert!(parse_callback("").is_err());
    }

    #[test]
    fn parse_callback_bad_hostname() {
        assert!(parse_callback("|deb|x86_64").is_err());
        assert!(parse_callback("host;evil|deb|x86_64").is_err());
    }

    #[test]
    fn parse_callback_bad_arch() {
        assert!(parse_callback("host|deb|i386").is_err());
    }

    #[test]
    fn parse_callback_bad_pkg_type() {
        assert!(parse_callback("host|msi|x86_64").is_err());
    }

    #[test]
    fn parse_callback_trims_whitespace() {
        let result = parse_callback("  myhost|deb|x86_64|ubuntu|22.04\n").unwrap();
        assert_eq!(result.hostname, "myhost");
    }

    // --- sanitize_filename ---

    #[test]
    fn filename_normal() {
        assert_eq!(sanitize_filename("falcon-sensor_7.0_amd64.deb"), "falcon-sensor_7.0_amd64.deb");
    }

    #[test]
    fn filename_path_traversal() {
        // Dots are allowed (for extensions), but '/' is stripped — no directory escape
        assert_eq!(sanitize_filename("../../etc/passwd"), "....etcpasswd");
    }

    #[test]
    fn filename_spaces() {
        assert_eq!(sanitize_filename("my file name.exe"), "myfilename.exe");
    }

    #[test]
    fn filename_special_chars() {
        assert_eq!(sanitize_filename("file;rm -rf /.exe"), "filerm-rf.exe");
    }
}

async fn serve_done(
    state: &AppState,
    req: axum::extract::Request,
    remote: &str,
    path: &str,
) -> Response {
    let body = match axum::body::to_bytes(req.into_body(), 4096).await {
        Ok(b) => String::from_utf8_lossy(&b).to_string(),
        Err(_) => {
            log_request("POST", path, remote, 400, "bad body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    // Parse: hostname|ok or hostname|error|message
    let parts: Vec<&str> = body.trim().splitn(3, '|').collect();
    if parts.len() < 2 {
        log_request("POST", path, remote, 400, "invalid done format");
        return StatusCode::BAD_REQUEST.into_response();
    }

    let hostname = parts[0];
    let result = parts[1];

    if result == "ok" {
        state.update_host_status(hostname, HostStatus::Installed);
        log_request("POST", path, remote, 200, &format!("done: {hostname} ok"));
        eprintln!("[host] {hostname} installed successfully");
    } else {
        let msg = if parts.len() > 2 {
            parts[2]
        } else {
            "unknown error"
        };
        state.update_host_status(hostname, HostStatus::Failed(msg.to_string()));
        log_request(
            "POST",
            path,
            remote,
            200,
            &format!("done: {hostname} error: {msg}"),
        );
        eprintln!("[host] {hostname} FAILED: {msg}");
    }

    StatusCode::OK.into_response()
}
