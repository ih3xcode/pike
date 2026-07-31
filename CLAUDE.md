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

The tree is organised by subsystem — one directory per area, each with its own
`mod.rs` naming what crosses its boundary. Dependencies run one way: `cli` wires
the subsystems together, `falcon` implements the ports declared by `sensors`,
and `common` depends on nothing.

**Entry point** (`main.rs` + `cli/`): `main.rs` only declares the modules and
calls `cli::run()`. `cli/mod.rs` holds the clap definitions and dispatch;
subcommands are `serve`, `gui` (also the default when no subcommand is given),
`update`, `service-install`, `service-uninstall`. `cli/serve.rs` composes the
application — resolve config, load local sensors, authenticate, build the caches
and `AppState`, then serve until timeout or download limit. `cli/banner.rs`
prints the startup summary.

**Configuration** (`config/`): `args.rs` (clap `ServeArgs`) + `file.rs` (TOML
`FileConfig`) merge into `ResolvedConfig` in `resolve.rs`, with precedence
flags > env (`PIKE_*`) > config file > defaults. All clap args are `Option<T>`
with no `default_value` — otherwise a clap default would be indistinguishable
from an explicit flag and always beat the config file. `defaults.rs` holds every
`DEFAULT_*` constant in one place; `validate.rs` holds `validate_token`.

**Sensors** (`sensors/`): `types.rs` has `Sensor`, `SensorType` and `SensorMeta`
— `SensorMeta` lives here rather than in `falcon/` so the caches and matching do
not depend on the API client. `ports.rs` declares `SensorLister` and
`SensorDownloader`, the only way the caches reach the outside world. They return
`BoxFuture` rather than `impl Future`: RPITIT traits are not dyn-compatible, and
`Arc<dyn …>` is what keeps a type parameter out of `AppState`. `matching.rs`
does distro/arch matching, `loading.rs` reads local files.

**Sensor caching** (`sensors/metadata_cache.rs`, `sensors/binary_store.rs`):
`MetadataCache` holds the API sensor list per platform with a TTL, serves a
stale snapshot when the API fails, and rate-limits retries after a failure.
`BinaryStore` is a disk cache keyed by sha256 with single-flight downloads,
checksum verification before publishing, atomic rename, and size-based eviction
(`plan_eviction`). API-downloaded binaries are never merged into
`local_sensors` — that separation is what keeps matching on fresh metadata.

**CrowdStrike API** (`falcon/`): `auth.rs` owns the OAuth2 client_credentials
flow and the token lifecycle (auto-refresh 60s before expiry), plus the cloud →
base URL mapping. `client.rs` has the endpoints (`get_ccid`, `list_sensors`,
`download_sensor`) and implements the `sensors` ports. Metadata calls carry
connect and total timeouts; `download_sensor` deliberately has no total timeout,
since sensors are hundreds of megabytes. Cloud defaults to `eu-1`.

**HTTP server** (`server/`): `state.rs` has `AppState` — token, CID,
`local_sensors: Vec<Sensor>` (immutable after startup), optional
`metadata`/`store` caches, host tracking (`Mutex<Vec<HostEntry>>`, 10,000 entry
cap with 25% eviction), the download counter, and base-URL construction. It
names no concrete API client, so tests build it over fakes. `routes.rs` mounts
5 routes (`/lin`, `/win`, `/s/{sha256}`, `/cb`, `/done`), optionally nested
under a `/{token}` prefix. Each route has its own file in `handlers/`, with
callback body parsing split out into `handlers/parse.rs`.

**Deployment flow**: Client fetches install script (`/lin` or `/win`) → script
calls back to `/cb` with `hostname|pkg_type|arch|distro_id|distro_version` →
pike matches a sensor and responds with `filename|sha256` → client downloads
from `/s/{sha256}`, verifies checksum, installs, reports result to `/done`.

**Sensor matching** (`sensors/matching.rs`): RPM matching uses distro tags
extracted from filenames (e.g. `.el9.x86_64.rpm`) with RHEL-family fallback
chains. DEB matching is by architecture only (x86_64→amd64, aarch64→arm64).
Windows uses a single multi-arch binary. Both local sensors and API metadata use
the same matching logic via `find_best_local_sensor` / `find_best_api_sensor`.

**Service installer** (`service/`): Gated behind `#[cfg(target_os = "linux")]`
except `units.rs`, which is platform-independent. `units.rs` generates the
systemd units as pure string functions (`main_unit`, `update_unit`,
`update_timer`) — `CAP_NET_BIND_SERVICE` is added only for ports below 1024, and
the cache dir is the sole `ReadWritePaths` entry. `wizard/` collects answers
(`prompts.rs`), validates them (`validate.rs`, plus a live API check) and
renders the config (`render.rs`); it returns an `InstallPlan` and never touches
the filesystem. `apply/` executes the plan: `preflight.rs` (root + systemd),
`fs.rs` (useradd, staged binary copy, 0640 config), `systemd.rs` (unit files,
`enable --now`). Auto-update is a separate root-owned timer, never the service
itself — under `ProtectSystem=strict` the service cannot rewrite its own binary,
and granting it that right would turn any RCE into persistence.

**Install scripts** (`scripts.rs` + `templates/`): `linux.sh` and `windows.ps1`
templates with placeholder substitution for base URL, CID, and cloud region.
Stays a single top-level file — it is a leaf subsystem too small for a directory.

**GUI** (`gui/`): eframe/egui application with three screens in `screens/`:
Config → Starting → Running. `state.rs` has the live `ConfigState`, `persist.rs`
the `SavedConfig` snapshot that survives a server restart. Has its own tokio
runtime. Screen transitions managed via the `Action` enum in `gui/mod.rs`.
Server lifecycle (start/stop) in `gui/lifecycle.rs`.

**Common** (`common/`): `error.rs` has the `AppError` enum (IO, HTTP, API
response, generic) using `thiserror`. `shutdown.rs` does graceful shutdown via
`tokio::sync::Notify`, triggered by timeout, download limit, or Ctrl-C.
`net.rs` detects local addresses, `token.rs` generates tokens.

**Updates** (`update/`): `check.rs` queries the GitHub release, `verify.rs`
checks the sha256 (accepting both bare hex and the `sha256sum` output format),
`apply.rs` downloads and replaces the binary — and refuses to do so when the
release has no `.sha256` asset.
