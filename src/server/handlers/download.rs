use axum::{
    extract::{OriginalUri, Path, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::server::helpers::{log_request, peer_addr};
use crate::server::state::AppState;

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

fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

async fn serve_sensor(state: &AppState, sha256: &str, remote: &str, path: &str) -> Response {
    if !is_valid_sha256(sha256) {
        log_request("GET", path, remote, 400, "malformed sha256");
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Local files live in memory, API-cached ones on disk
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

    // Deliberately `File::open` rather than `store.ensure`: otherwise any
    // 404 on valid hex would trigger a download from the API, turning this
    // route into a cheap amplifier of outbound traffic.
    let file_path = store.path_for(sha256);
    // Only ever hand out a regular file. The cache directory is 0700, so a
    // planted symlink should be impossible — but what goes over this route is
    // executed as root on the far end, and the check costs one stat.
    if !matches!(tokio::fs::symlink_metadata(&file_path).await, Ok(m) if m.is_file()) {
        log_request("GET", path, remote, 404, "sensor not in cache");
        return StatusCode::NOT_FOUND.into_response();
    }
    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => {
            log_request("GET", path, remote, 404, "sensor not in cache");
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    let size = file.metadata().await.map(|m| m.len()).ok();

    // The counter only moves when a real file is served — otherwise a run
    // of 404s would eat the whole --max-downloads budget
    let (count, extra) = count_download(state);
    log_request("GET", path, remote, 200, &extra);
    match size {
        Some(n) => eprintln!("[sensor] Serving {sha256} to {remote} ({n} bytes, from cache)"),
        None => eprintln!("[sensor] Serving {sha256} to {remote} (size unknown, from cache)"),
    }
    maybe_stop(state, count);

    let stream = tokio_util::io::ReaderStream::new(file);
    let mut resp = axum::body::Body::from_stream(stream).into_response();
    // Only set the header when the size is actually known: hyper trusts an
    // explicit content-length and truncates the body to it, so a `0` fallback
    // would hand the client an empty file under a 200
    if let Some(n) = size
        && let Ok(value) = HeaderValue::from_str(&n.to_string())
    {
        resp.headers_mut().insert("content-length", value);
    }
    octet_stream(resp)
}

/// Counts one successful download; returns its number and a log fragment.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::test_support::state;

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
            "uppercase is not accepted"
        );
        assert!(!is_valid_sha256(&"g".repeat(64)));
    }

    #[tokio::test]
    async fn unknown_sha256_does_not_consume_the_download_budget() {
        let mut st = state(None, None);
        st.max_downloads = 3;

        let resp = serve_sensor(&st, &"b".repeat(64), "10.0.0.9", "/s/x").await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            st.download_count
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a 404 must not spend the --max-downloads budget"
        );
    }

    #[tokio::test]
    async fn malformed_sha256_is_rejected_before_any_lookup() {
        let st = state(None, None);
        let resp = serve_sensor(&st, "../../etc/passwd", "10.0.0.9", "/s/x").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
