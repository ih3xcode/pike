use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::sensors::matching::{find_best_api_sensor, find_best_local_sensor};
use crate::sensors::SensorType;
use crate::server::helpers::{log_request, peer_addr};
use crate::server::state::{AppState, HostEntry, HostStatus};

use super::parse::parse_callback;

pub async fn callback(
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

    serve_callback(&state, &body, &remote, &path).await
}

async fn serve_callback(state: &AppState, body: &str, remote: &str, path: &str) -> Response {
    let info = match parse_callback(body) {
        Ok(i) => i,
        Err(reason) => {
            log_request("POST", path, remote, 400, reason);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

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
        &format!(
            "cb: {} {}/{}{distro_info}",
            info.hostname, info.pkg_type, info.arch
        ),
    );
    eprintln!(
        "[host] {} registered (ip={remote}, type={}, arch={}{distro_info})",
        info.hostname, info.pkg_type, info.arch
    );

    // 1. Explicitly supplied files win — that is a deliberate version pin
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

    // 2. Otherwise: a fresh list from the API and the sha256-keyed cache
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

    // The /s/{sha256} route only accepts lowercase — normalise here, at the
    // metadata boundary, so the answer and the cached file name agree
    let sha256 = meta.sha256.to_ascii_lowercase();

    if let Err(e) = store.ensure(&sha256, meta.file_size).await {
        eprintln!("[host] {}: cannot prepare sensor — {e}", info.hostname);
        state.update_host_status(&info.hostname, HostStatus::Failed("sensor unavailable".into()));
        log_request("POST", path, remote, 502, "sensor download failed");
        return (StatusCode::BAD_GATEWAY, "sensor download failed").into_response();
    }

    state.update_host_status(&info.hostname, HostStatus::SensorReady);
    format!("{}|{}", meta.name, sha256).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::AppError;
    use crate::sensors::ports::{BoxFuture, BoxReader, SensorDownloader, SensorLister};
    use crate::sensors::types::SensorMeta;
    use crate::sensors::{BinaryStore, MetadataCache};
    use crate::server::state::test_support::state;
    use sha2::{Digest, Sha256};
    use std::time::Duration;

    const PAYLOAD: &[u8] = b"pretend this is a falcon sensor";

    fn payload_sha() -> String {
        hex::encode(Sha256::digest(PAYLOAD))
    }

    struct Api {
        metas: Vec<SensorMeta>,
        list_fails: bool,
    }

    impl SensorLister for Api {
        fn list<'a>(
            &'a self,
            _platform: &'a str,
        ) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>> {
            Box::pin(async move {
                if self.list_fails {
                    Err(AppError::Other("api down".into()))
                } else {
                    Ok(self.metas.clone())
                }
            })
        }
    }

    impl SensorDownloader for Api {
        fn fetch<'a>(&'a self, _sha256: &'a str) -> BoxFuture<'a, Result<BoxReader<'a>, AppError>> {
            Box::pin(async move { Ok(Box::pin(std::io::Cursor::new(PAYLOAD)) as BoxReader<'a>) })
        }
    }

    fn deb_meta(name: &str) -> SensorMeta {
        SensorMeta {
            name: name.to_string(),
            // Uppercase on purpose: that is how the API returns the sha,
            // while the /s/{sha256} route only accepts lowercase
            sha256: payload_sha().to_uppercase(),
            platform: "linux".into(),
            os: "Ubuntu".into(),
            file_type: "deb".into(),
            file_size: PAYLOAD.len() as u64,
            version: "7.20".into(),
            architectures: vec!["x86_64".into()],
        }
    }

    /// State backed by a fake API. Also returns the cache directory, which
    /// must be kept alive for the duration of the test.
    fn state_with_api(api: Arc<Api>) -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut st = state(None, None);
        st.metadata = Some(Arc::new(MetadataCache::new(
            api.clone(),
            Duration::from_secs(60),
        )));
        st.store = Some(Arc::new(BinaryStore::new(
            api,
            dir.path().to_path_buf(),
            u64::MAX,
        )));
        (st, dir)
    }

    async fn body_of(resp: Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn api_sensor_is_downloaded_and_announced_by_sha256() {
        let api = Arc::new(Api {
            metas: vec![deb_meta("falcon-sensor_7.20_amd64.deb")],
            list_fails: false,
        });
        let (st, dir) = state_with_api(api);

        let resp = serve_callback(&st, "host1|deb|x86_64|ubuntu|22.04", "10.0.0.5", "/cb").await;

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            body_of(resp).await,
            format!("falcon-sensor_7.20_amd64.deb|{}", payload_sha()),
            "the client must be given the sha in lowercase"
        );
        assert!(
            dir.path().join(payload_sha()).exists(),
            "the binary should have landed in the cache under its sha"
        );
        let hosts = st.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::SensorReady));
    }

    #[tokio::test]
    async fn unavailable_sensor_list_answers_503() {
        let api = Arc::new(Api {
            metas: vec![],
            list_fails: true,
        });
        let (st, _dir) = state_with_api(api);

        let resp = serve_callback(&st, "host1|deb|x86_64|ubuntu|22.04", "10.0.0.5", "/cb").await;

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let hosts = st.hosts.lock().unwrap();
        assert!(matches!(hosts[0].status, HostStatus::Failed(_)));
    }

    #[tokio::test]
    async fn no_matching_arch_answers_404() {
        let api = Arc::new(Api {
            metas: vec![deb_meta("falcon-sensor_7.20_amd64.deb")],
            list_fails: false,
        });
        let (st, _dir) = state_with_api(api);

        let resp = serve_callback(&st, "host1|deb|aarch64|ubuntu|22.04", "10.0.0.5", "/cb").await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn without_api_and_local_sensors_answers_404() {
        let st = state(None, None);
        let resp = serve_callback(&st, "host1|deb|x86_64|ubuntu|22.04", "10.0.0.5", "/cb").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn malformed_body_is_rejected() {
        let st = state(None, None);
        let resp = serve_callback(&st, "garbage", "10.0.0.5", "/cb").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            st.hosts.lock().unwrap().is_empty(),
            "the host should not have been registered"
        );
    }
}
