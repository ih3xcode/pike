use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::server::helpers::{log_request, peer_addr};
use crate::server::state::{AppState, HostStatus};

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

/// Тіло: `hostname|ok` або `hostname|error|повідомлення`.
fn serve_done(state: &AppState, body: &str, remote: &str, path: &str) -> Response {
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
}
