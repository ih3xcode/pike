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

    // Свідомо `File::open`, а не `store.ensure`: інакше будь-який 404 на
    // валідний hex запускав би завантаження з API, і маршрут став би
    // дешевим підсилювачем зовнішнього трафіку.
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
            "верхній регістр не приймаємо"
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
            "404 не має витрачати ліміт --max-downloads"
        );
    }

    #[tokio::test]
    async fn malformed_sha256_is_rejected_before_any_lookup() {
        let st = state(None, None);
        let resp = serve_sensor(&st, "../../etc/passwd", "10.0.0.9", "/s/x").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
