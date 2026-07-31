use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::server::helpers::{log_request, peer_addr};
use crate::server::state::{AppState, HostStatus};

use super::parse::is_valid_hostname;

/// The longest error message we are willing to display.
const MAX_MESSAGE_LEN: usize = 200;

pub async fn done(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    req: axum::extract::Request,
) -> Response {
    let remote = peer_addr(&req);
    let path = uri.path().to_string();

    let body = match axum::body::to_bytes(req.into_body(), 4096).await {
        Ok(b) => String::from_utf8_lossy(&b).to_string(),
        Err(_) => {
            log_request("POST", &path, &remote, 400, "bad body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    serve_done(&state, &body, &remote, &path)
}

/// Body: `hostname|ok` or `hostname|error|message`.
fn serve_done(state: &AppState, body: &str, remote: &str, path: &str) -> Response {
    let parts: Vec<&str> = body.trim().splitn(3, '|').collect();
    if parts.len() < 2 {
        log_request("POST", path, remote, 400, "invalid done format");
        return StatusCode::BAD_REQUEST.into_response();
    }

    let hostname = parts[0];
    // Same rules as /cb: the hostname and the message end up in the journal,
    // so a newline here would let a caller forge someone else's line
    if !is_valid_hostname(hostname) {
        log_request("POST", path, remote, 400, "invalid hostname");
        return StatusCode::BAD_REQUEST.into_response();
    }
    let result = parts[1];

    if result == "ok" {
        state.update_host_status(hostname, HostStatus::Installed);
        log_request("POST", path, remote, 200, &format!("done: {hostname} ok"));
        eprintln!("[host] {hostname} installed successfully");
    } else {
        let msg = sanitize_message(parts.get(2).copied().unwrap_or("unknown error"));
        state.update_host_status(hostname, HostStatus::Failed(msg.clone()));
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

/// Drops control characters (newlines above all, which could be used to
/// forge a journal entry) and truncates to a sane length.
fn sanitize_message(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_MESSAGE_LEN)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "unknown error".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::test_support::state;
    use crate::server::state::HostEntry;

    fn state_with_host(hostname: &str) -> AppState {
        let st = state(None, None);
        st.push_host(HostEntry {
            hostname: hostname.into(),
            platform: "deb".into(),
            arch: "x86_64".into(),
            ip: "10.0.0.5".into(),
            status: HostStatus::SensorReady,
            time: chrono::Local::now(),
        });
        st
    }

    #[test]
    fn ok_marks_the_host_installed() {
        let st = state_with_host("host1");
        let resp = serve_done(&st, "host1|ok", "10.0.0.5", "/done");
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(matches!(
            st.hosts.lock().unwrap()[0].status,
            HostStatus::Installed
        ));
    }

    #[test]
    fn error_keeps_the_reported_message() {
        let st = state_with_host("host1");
        serve_done(&st, "host1|error|dpkg exited 1", "10.0.0.5", "/done");
        let hosts = st.hosts.lock().unwrap();
        let HostStatus::Failed(msg) = &hosts[0].status else {
            panic!("expected Failed");
        };
        assert_eq!(msg, "dpkg exited 1");
    }

    #[test]
    fn too_few_fields_is_a_bad_request() {
        let st = state_with_host("host1");
        let resp = serve_done(&st, "host1", "10.0.0.5", "/done");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn hostname_with_a_newline_is_rejected() {
        let st = state_with_host("host1");
        let resp = serve_done(&st, "host1\n[host] fake|ok", "10.0.0.5", "/done");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(matches!(
            st.hosts.lock().unwrap()[0].status,
            HostStatus::SensorReady
        ));
    }

    #[test]
    fn control_characters_are_stripped_from_the_message() {
        let st = state_with_host("host1");
        serve_done(
            &st,
            "host1|error|dpkg failed\n[host] host1 installed successfully",
            "10.0.0.5",
            "/done",
        );
        let hosts = st.hosts.lock().unwrap();
        let HostStatus::Failed(msg) = &hosts[0].status else {
            panic!("expected Failed");
        };
        assert!(!msg.contains('\n'), "the newline should be gone: {msg:?}");
    }

    #[test]
    fn overlong_message_is_truncated() {
        let st = state_with_host("host1");
        let long = "x".repeat(1000);
        serve_done(&st, &format!("host1|error|{long}"), "10.0.0.5", "/done");
        let hosts = st.hosts.lock().unwrap();
        let HostStatus::Failed(msg) = &hosts[0].status else {
            panic!("expected Failed");
        };
        assert_eq!(msg.chars().count(), MAX_MESSAGE_LEN);
    }

    #[test]
    fn blank_message_falls_back_to_a_placeholder() {
        let st = state_with_host("host1");
        serve_done(&st, "host1|error|   ", "10.0.0.5", "/done");
        let hosts = st.hosts.lock().unwrap();
        let HostStatus::Failed(msg) = &hosts[0].status else {
            panic!("expected Failed");
        };
        assert_eq!(msg, "unknown error");
    }
}
