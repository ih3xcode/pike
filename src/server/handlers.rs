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
    Path(sha256): Path<String>,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();
    serve_sensor(&state, &sha256, &remote, &path).await
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
    let script = scripts::generate_linux_script(&base_url, &state.cid, state.cloud.as_deref(), state.tags.as_deref());

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
    let script = scripts::generate_windows_script(&base_url, &state.cid, state.cloud.as_deref(), state.tags.as_deref());

    log_request("GET", path, remote, 200, "windows script");
    eprintln!("[script] Served windows install script to {remote} (base_url={base_url})");

    let mut resp = script.into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
}

fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

async fn serve_sensor(state: &AppState, sha256: &str, remote: &str, path: &str) -> Response {
    if !is_valid_sha256(sha256) {
        log_request("GET", path, remote, 400, "malformed sha256");
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Локальні файли лежать у памʼяті, кешовані з API — на диску
    if let Some(sensor) = state.local_sensors.iter().find(|s| s.sha256 == sha256) {
        let (count, extra) = count_download(state);
        log_request("GET", path, remote, 200, &extra);
        eprintln!(
            "[sensor] Serving {} to {remote} ({} bytes)",
            sensor.filename,
            sensor.data.len()
        );
        maybe_stop(state, count);
        return octet_stream(sensor.data.clone().into_response());
    }

    let Some(store) = &state.store else {
        log_request("GET", path, remote, 404, "sensor not found");
        return StatusCode::NOT_FOUND.into_response();
    };

    let file_path = store.path_for(sha256);
    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => {
            log_request("GET", path, remote, 404, "sensor not in cache");
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    let size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    // Лічильник рухається лише коли віддаємо справжній файл — інакше
    // серія 404 з'їдала б увесь ліміт --max-downloads
    let (count, extra) = count_download(state);
    log_request("GET", path, remote, 200, &extra);
    eprintln!("[sensor] Serving {sha256} to {remote} ({size} bytes, from cache)");
    maybe_stop(state, count);

    let stream = tokio_util::io::ReaderStream::new(file);
    let mut resp = axum::body::Body::from_stream(stream).into_response();
    resp.headers_mut().insert(
        "content-length",
        HeaderValue::from_str(&size.to_string()).unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    octet_stream(resp)
}

/// Зараховує одне успішне завантаження; повертає його номер і рядок для логу.
fn count_download(state: &AppState) -> (u32, String) {
    let count = state
        .download_count
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let extra = if state.max_downloads > 0 {
        format!("download {}/{}", count, state.max_downloads)
    } else {
        format!("download {count}")
    };
    (count, extra)
}

fn octet_stream(mut resp: Response) -> Response {
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    resp
}

fn maybe_stop(state: &AppState, count: u32) {
    if state.max_downloads > 0 && count >= state.max_downloads {
        eprintln!(
            "[server] Download limit reached ({count}/{}), shutting down",
            state.max_downloads
        );
        state.shutdown_notify.notify_one();
    }
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

    // 1. Явно передані файли мають пріоритет — це свідомий пін версії
    if let Some(sensor) = find_best_local_sensor(
        &state.local_sensors,
        info.target_type,
        &info.arch,
        &info.distro_id,
        &info.distro_version,
    ) {
        eprintln!(
            "[host] {}: matched local sensor {}",
            info.hostname, sensor.filename
        );
        state.update_host_status(&info.hostname, HostStatus::SensorReady);
        return format!("{}|{}", sensor.filename, sensor.sha256).into_response();
    }

    // 2. Інакше — свіжий список з API і кеш за sha256
    let (Some(metadata), Some(store)) = (&state.metadata, &state.store) else {
        eprintln!(
            "[host] {}: no local sensor and no API configured",
            info.hostname
        );
        state.update_host_status(&info.hostname, HostStatus::Failed("no matching sensor".into()));
        log_request("POST", path, remote, 404, "no sensor available");
        return (StatusCode::NOT_FOUND, "no matching sensor available").into_response();
    };

    let platform = match info.target_type {
        SensorType::Deb | SensorType::Rpm => "linux",
        SensorType::WindowsExe => "windows",
    };

    let metas = match metadata.get(platform).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[host] {}: sensor list unavailable — {e}", info.hostname);
            state.update_host_status(
                &info.hostname,
                HostStatus::Failed("sensor list unavailable".into()),
            );
            log_request("POST", path, remote, 503, "sensor list unavailable");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "sensor list unavailable — CrowdStrike API unreachable",
            )
                .into_response();
        }
    };

    let file_type = match info.target_type {
        SensorType::Deb => "deb",
        SensorType::Rpm => "rpm",
        SensorType::WindowsExe => "exe",
    };

    let Some(meta) = find_best_api_sensor(
        &metas,
        file_type,
        &info.arch,
        &info.distro_id,
        &info.distro_version,
    ) else {
        eprintln!(
            "[host] {} FAILED: no matching sensor available",
            info.hostname
        );
        state.update_host_status(&info.hostname, HostStatus::Failed("no matching sensor".into()));
        log_request("POST", path, remote, 404, "no matching sensor");
        return (StatusCode::NOT_FOUND, "no matching sensor available").into_response();
    };

    eprintln!(
        "[host] {}: matched {} (os={})",
        info.hostname, meta.name, meta.os
    );

    if let Err(e) = store.ensure(&meta.sha256).await {
        eprintln!("[host] {}: cannot prepare sensor — {e}", info.hostname);
        state.update_host_status(&info.hostname, HostStatus::Failed("sensor unavailable".into()));
        log_request("POST", path, remote, 502, "sensor download failed");
        return (StatusCode::BAD_GATEWAY, "sensor download failed").into_response();
    }

    state.update_host_status(&info.hostname, HostStatus::SensorReady);
    format!("{}|{}", meta.name, meta.sha256).into_response()
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

    // --- is_valid_sha256 ---

    #[test]
    fn sha256_path_valid() {
        assert!(is_valid_sha256(&"a".repeat(64)));
        assert!(is_valid_sha256("0123456789abcdef".repeat(4).as_str()));
    }

    #[test]
    fn sha256_path_rejects_wrong_length() {
        assert!(!is_valid_sha256(&"a".repeat(63)));
        assert!(!is_valid_sha256(&"a".repeat(65)));
        assert!(!is_valid_sha256(""));
    }

    #[test]
    fn sha256_path_rejects_traversal_and_non_hex() {
        assert!(!is_valid_sha256("../../etc/passwd"));
        assert!(
            !is_valid_sha256(&"A".repeat(64)),
            "верхній регістр не приймаємо"
        );
        assert!(!is_valid_sha256(&"g".repeat(64)));
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
