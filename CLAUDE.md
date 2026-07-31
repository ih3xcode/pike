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

**Entry point** (`main.rs`): Parses CLI args with clap. Three subcommands: `serve` (HTTP server), `gui` (also the default when no subcommand is given), `update`. `run_serve` resolves the config, authenticates, builds `AppState`, starts the HTTP server, and blocks until timeout or download limit.

**Configuration** (`config.rs`): `ServeArgs` (clap) + `FileConfig` (TOML) merge into `ResolvedConfig` with precedence flags > env (`PIKE_*`) > config file > defaults. All clap args are `Option<T>` with no `default_value` — otherwise a clap default would be indistinguishable from an explicit flag and always beat the config file.

**Shared state** (`types.rs`): `AppState` is the central `Arc`-wrapped struct holding token, CID, `local_sensors: Vec<Sensor>` (immutable after startup), optional `metadata`/`store` caches, host tracking (`Mutex<Vec<HostEntry>>`), and the download counter. Hosts list has a 10,000 entry cap with 25% eviction.

**Sensor caching** (`sensor_store.rs`): `MetadataCache` holds the API sensor list per platform with a TTL, serves a stale snapshot when the API fails, and rate-limits retries after a failure. `BinaryStore` is a disk cache keyed by sha256 with single-flight downloads, checksum verification before publishing, atomic rename, and size-based eviction (`plan_eviction`). API-downloaded binaries are never merged into `local_sensors` — that separation is what keeps matching on fresh metadata.

**HTTP server** (`server/`): Axum-based with 5 routes (`/lin`, `/win`, `/s/{sha256}`, `/cb`, `/done`). Routes are optionally nested under `/{token}` prefix when auth is enabled. Handlers are in `server/handlers.rs`, helper functions in `server/helpers.rs`.

**Deployment flow**: Client fetches install script (`/lin` or `/win`) → script calls back to `/cb` with `hostname|pkg_type|arch|distro_id|distro_version` → pike matches a sensor and responds with `filename|sha256` → client downloads from `/s/{sha256}`, verifies checksum, installs, reports result to `/done`.

**Sensor matching** (`sensor_match.rs`): RPM matching uses distro tags extracted from filenames (e.g. `.el9.x86_64.rpm`) with RHEL-family fallback chains. DEB matching is by architecture only (x86_64→amd64, aarch64→arm64). Windows uses a single multi-arch binary. Both local sensors and API metadata use the same matching logic via `find_best_local_sensor` / `find_best_api_sensor`.

**CrowdStrike API** (`falcon_api.rs`): `FalconClient` handles OAuth2 client_credentials flow with auto-refresh (60s before expiry). Provides `get_ccid()`, `list_sensors()`, `download_sensor()`. API-fetched sensors are cached in the shared `sensors` RwLock. Cloud regions map to different API base URLs.

**Install scripts** (`scripts.rs` + `templates/`): `linux.sh` and `windows.ps1` templates with placeholder substitution for base URL, CID, and cloud region.

**GUI** (`gui/`): eframe/egui application with three screens: Config → Starting → Running. Has its own tokio runtime. Screen transitions managed via `Action` enum in `gui/mod.rs`. Server lifecycle (start/stop) in `gui/lifecycle.rs`.

**Error handling** (`error.rs`): `AppError` enum with variants for IO, HTTP, API response, and generic errors. Uses `thiserror` for Display derivation.

**Shutdown** (`shutdown.rs`): Graceful shutdown via `tokio::sync::Notify`, triggered by timeout, download limit, or Ctrl-C.
