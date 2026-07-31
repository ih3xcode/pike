use std::sync::Arc;
use std::time::Instant;

use crate::falcon_api::FalconClient;
use crate::types::AppState;

use super::config::*;
use super::running::RunningState;
use super::starting::{InitResult, StartingState};
use super::Screen;

impl super::MindeliverApp {
    /// Phase 1: validate config, load local sensors, spawn async init task.
    /// Transitions Config -> Starting (no blocking network I/O on UI thread).
    pub(super) fn begin_starting(&mut self) {
        let Screen::Config(config) = &mut self.screen else {
            return;
        };

        let port: u16 = config.port.parse().unwrap_or(8080);
        let timeout: u64 = config.timeout.parse().unwrap_or(30);
        let max_downloads: u32 = config.max_downloads.parse().unwrap_or(0);
        let cloud = Some(CLOUD_OPTIONS[config.cloud_idx].to_string());

        eprintln!("[gui] Starting server...");
        let sensors = if config.sensor_paths.is_empty() {
            eprintln!("[gui] No local sensor files");
            Vec::new()
        } else {
            eprintln!(
                "[gui] Loading {} sensor file(s)...",
                config.sensor_paths.len()
            );
            match crate::util::load_sensors(&config.sensor_paths) {
                Ok(s) => {
                    for sensor in &s {
                        eprintln!(
                            "[gui]   Loaded {} ({} bytes, sha256={})",
                            sensor.filename,
                            sensor.data.len(),
                            &sensor.sha256[..12]
                        );
                    }
                    s
                }
                Err(e) => {
                    eprintln!("[gui] ERROR loading sensors: {e}");
                    config.error = Some(e.to_string());
                    return;
                }
            }
        };

        let has_api_creds =
            !config.client_id.trim().is_empty() && !config.client_secret.trim().is_empty();

        let addr = config.available_addrs[config.selected_addr_idx]
            .1
            .clone();

        let public_url =
            if config.custom_url_enabled && !config.custom_url.trim().is_empty() {
                Some(
                    config
                        .custom_url
                        .trim()
                        .trim_end_matches('/')
                        .to_string(),
                )
            } else {
                None
            };

        let saved_config = make_saved_config(config);

        // Spawn the (potentially slow) API auth work off the UI thread.
        let client_id = config.client_id.trim().to_string();
        let client_secret = config.client_secret.trim().to_string();
        let cloud_for_task = cloud.clone();

        let init_handle = self.runtime.spawn(async move {
            if !has_api_creds {
                eprintln!("[gui] No API credentials, local-only mode");
                return Ok(InitResult {
                    falcon_client: None,
                    api_cid: None,
                });
            }

            eprintln!("[gui] API credentials provided, authenticating...");
            let client = FalconClient::new(
                &client_id,
                &client_secret,
                cloud_for_task.as_deref(),
            )
            .await?;

            let api_cid = client.get_ccid().await.ok();
            eprintln!(
                "[gui] API connected, CID from API: {}",
                api_cid.as_deref().unwrap_or("(not fetched)")
            );

            Ok(InitResult {
                falcon_client: Some(client),
                api_cid,
            })
        });

        let cid_explicit = config.cid.trim().to_string();

        let tags = crate::scripts::resolve_tags(
            Some(&config.tags),
            !config.no_default_tag,
        );

        self.screen = Screen::Starting(StartingState {
            init_handle,
            sensors,
            port,
            timeout,
            max_downloads,
            cloud,
            cid_explicit,
            auth_enabled: config.auth_enabled,
            tags,
            addr,
            public_url,
            saved_config,
        });
    }

    /// Phase 2: async init task finished — build AppState and start the server.
    pub(super) fn finish_starting(&mut self) {
        // Guard: only proceed if task is actually finished (prevents blocking UI thread)
        let Screen::Starting(ref starting) = self.screen else {
            return;
        };
        if !starting.init_handle.is_finished() {
            return;
        }

        // Take ownership of the Starting state.
        let prev = std::mem::replace(
            &mut self.screen,
            Screen::Config(new_config_state()), // placeholder
        );
        let Screen::Starting(starting) = prev else {
            return;
        };

        // Collect the task result (already finished, won't block).
        let init_result = match self.runtime.block_on(starting.init_handle) {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                eprintln!("[gui] API auth failed: {e}");
                let mut config = config_from_saved(starting.saved_config);
                config.error = Some(format!("API auth failed: {e}"));
                self.screen = Screen::Config(config);
                return;
            }
            Err(e) => {
                eprintln!("[gui] Init task panicked: {e}");
                let mut config = config_from_saved(starting.saved_config);
                config.error = Some(format!("Init failed: {e}"));
                self.screen = Screen::Config(config);
                return;
            }
        };

        // Resolve CID
        let cid = if !starting.cid_explicit.is_empty() {
            eprintln!("[gui] Using explicit CID: {}", starting.cid_explicit);
            starting.cid_explicit
        } else if let Some(c) = init_result.api_cid {
            eprintln!("[gui] Using CID from API: {c}");
            c
        } else {
            eprintln!("[gui] ERROR: no CID available");
            let mut config = config_from_saved(starting.saved_config);
            config.error = Some("CID is required (provide it or use API credentials)".into());
            self.screen = Screen::Config(config);
            return;
        };

        let token = if starting.auth_enabled {
            Some(crate::util::generate_token())
        } else {
            None
        };
        let shutdown_notify = Arc::new(tokio::sync::Notify::new());

        let has_api = init_result.falcon_client.is_some();
        let bind_ip = starting.addr.clone();

        let falcon = init_result.falcon_client.map(std::sync::Arc::new);

        let cache_dir = crate::config::default_cache_dir();
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            eprintln!(
                "[gui] WARNING: cannot create cache dir {}: {e}",
                cache_dir.display()
            );
        }

        let metadata = falcon.clone().map(|c| {
            std::sync::Arc::new(crate::sensor_store::MetadataCache::new(
                c,
                std::time::Duration::from_secs(60 * 60),
            ))
        });
        let store = falcon.map(|c| {
            std::sync::Arc::new(crate::sensor_store::BinaryStore::new(
                c,
                cache_dir,
                21_474_836_480,
            ))
        });

        let app_state = Arc::new(AppState {
            token,
            cid,
            cloud: starting.cloud,
            addr: starting.addr,
            port: starting.port,
            public_url: starting.public_url,
            local_sensors: starting.sensors,
            metadata,
            store,
            download_count: std::sync::atomic::AtomicU32::new(0),
            max_downloads: starting.max_downloads,
            shutdown_notify: shutdown_notify.clone(),
            hosts: std::sync::Mutex::new(Vec::new()),
            tags: starting.tags,
        });

        let cloud_str = app_state.cloud.as_deref().unwrap_or("(none)");
        eprintln!(
            "[gui] Configuration: port={}, timeout={}m, max_downloads={}, cloud={cloud_str}",
            starting.port, starting.timeout, starting.max_downloads
        );
        eprintln!(
            "[gui] Token: {}",
            app_state.token.as_deref().unwrap_or("(disabled)")
        );
        eprintln!(
            "[gui] Advertised address: {}:{}",
            app_state.addr, starting.port
        );
        if let Some(url) = &app_state.public_url {
            eprintln!("[gui] Public URL: {url}");
        }

        let port = starting.port;
        let timeout = starting.timeout;
        let state_clone = app_state.clone();
        let bind_addr: std::net::SocketAddr =
            format!("{}:{}", bind_ip, port).parse().unwrap();
        let handle = self.runtime.spawn(async move {
            if let Err(e) = crate::server::run_server(
                state_clone,
                bind_addr,
                timeout,
                shutdown_notify,
                false,
            )
            .await
            {
                eprintln!("[server] {e}");
            }
        });

        self.screen = Screen::Running(RunningState {
            app_state,
            started_at: Instant::now(),
            timeout_minutes: timeout,
            server_handle: handle,
            shutdown_triggered: false,
            copied_at: std::collections::HashMap::new(),
            saved_config: starting.saved_config,
            has_api,
        });
    }
}
