# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Pike

Pike is a CrowdStrike Falcon sensor deployment tool. It serves installation scripts and sensor binaries over HTTP so target hosts can install the Falcon sensor with a single curl/irm command. Supports local sensor files and on-demand retrieval from the CrowdStrike API. Works on Linux (deb/rpm) and Windows (exe). Has both CLI and GUI modes.

## Build & Test

```bash
cargo build --release       # release binary at target/release/pike
cargo test                  # run all tests
cargo test sensor_match     # run tests in a specific module
cargo test test_name        # run a single test by name
```

Requires Rust 2024 edition. Release profile uses LTO, strip, opt-level "z", and panic=abort for small binaries.

## Architecture

**Entry point** (`main.rs`): Parses CLI args with clap. Launches GUI mode when no arguments are provided (or `--gui`), otherwise runs CLI mode which builds `AppState`, starts the HTTP server, and blocks until timeout or download limit.

**Shared state** (`types.rs`): `AppState` is the central `Arc`-wrapped struct holding token, CID, sensors (`RwLock<Vec<Sensor>>`), host tracking (`Mutex<Vec<HostEntry>>`), download counter, and optional `FalconClient`. Hosts list has a 10,000 entry cap with 25% eviction.

**HTTP server** (`server/`): Axum-based with 5 routes (`/lin`, `/win`, `/s/{filename}`, `/cb`, `/done`). Routes are optionally nested under `/{token}` prefix when auth is enabled. Handlers are in `server/handlers.rs`, helper functions in `server/helpers.rs`.

**Deployment flow**: Client fetches install script (`/lin` or `/win`) → script calls back to `/cb` with `hostname|pkg_type|arch|distro_id|distro_version` → pike matches a sensor and responds with `filename|sha256` → client downloads from `/s/{filename}`, verifies checksum, installs, reports result to `/done`.

**Sensor matching** (`sensor_match.rs`): RPM matching uses distro tags extracted from filenames (e.g. `.el9.x86_64.rpm`) with RHEL-family fallback chains. DEB matching is by architecture only (x86_64→amd64, aarch64→arm64). Windows uses a single multi-arch binary. Both local sensors and API metadata use the same matching logic via `find_best_local_sensor` / `find_best_api_sensor`.

**CrowdStrike API** (`falcon_api.rs`): `FalconClient` handles OAuth2 client_credentials flow with auto-refresh (60s before expiry). Provides `get_ccid()`, `list_sensors()`, `download_sensor()`. API-fetched sensors are cached in the shared `sensors` RwLock. Cloud regions map to different API base URLs.

**Install scripts** (`scripts.rs` + `templates/`): `linux.sh` and `windows.ps1` templates with placeholder substitution for base URL, CID, and cloud region.

**GUI** (`gui/`): eframe/egui application with three screens: Config → Starting → Running. Has its own tokio runtime. Screen transitions managed via `Action` enum in `gui/mod.rs`. Server lifecycle (start/stop) in `gui/lifecycle.rs`.

**Error handling** (`error.rs`): `AppError` enum with variants for IO, HTTP, API response, and generic errors. Uses `thiserror` for Display derivation.

**Shutdown** (`shutdown.rs`): Graceful shutdown via `tokio::sync::Notify`, triggered by timeout, download limit, or Ctrl-C.
