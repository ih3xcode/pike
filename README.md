# Pike

CrowdStrike Falcon sensor deployment tool. Serves installation scripts and sensor binaries over HTTP so target hosts can install with a single command. Supports local sensor files and on-demand retrieval from the CrowdStrike API.

Works on Linux (deb/rpm) and Windows (exe). Has both CLI and GUI modes.

## Screenshots

| Main screen | Files mode | Running server |
|---|---|---|
| ![Main screen](screenshots/main.png) | ![Files mode](screenshots/files.png) | ![Running server](screenshots/running.png) |

## Build

Requires Rust 2024 edition.

```
cargo build --release
```

Binary: `target/release/pike`

## Usage

### GUI mode

Starts automatically when no CLI arguments are provided:

```
pike
```

Or force it:

```
pike --gui
```

### CLI mode

```
pike --sensor ./falcon-sensor.deb --cid <CID>
```

With CrowdStrike API (sensors fetched on demand):

```
pike --client-id <ID> --client-secret <SECRET> --cid <CID> --cloud us-1
```

The API client only needs the **Sensor Download** scope with **Read** permission.

### Flags

| Flag | Description |
|---|---|
| `--sensor <PATH>` | Local sensor file(s), can be specified multiple times |
| `--cid <CID>` | CrowdStrike Customer ID |
| `--client-id <ID>` | API Client ID (enables on-demand sensor download) |
| `--client-secret <SECRET>` | API Client Secret |
| `--cloud <CLOUD>` | Cloud region: us-1, us-2, eu-1, us-gov-1, us-gov-2 |
| `--addr <ADDR>` | Advertised address for one-liners (auto-detected) |
| `--port <PORT>` | HTTP port (default: 8080) |
| `--bind <ADDR>` | Bind address (default: 0.0.0.0) |
| `--timeout <MIN>` | Deployment timeout in minutes (default: 30) |
| `--max-downloads <N>` | Shut down after N downloads, 0 = unlimited |
| `--no-auth` | Disable token authentication |
| `--gui` | Force GUI mode |

## One-liners

After starting, pike prints ready-to-use commands:

Linux:
```
curl -fsS http://<server>:<port>/<token>/lin | sudo bash
```

Windows (run as Administrator):
```
irm http://<server>:<port>/<token>/win | iex
```

Without auth (`--no-auth`), the `/<token>` prefix is omitted.

## How it works

1. Pike starts an HTTP server and serves installation scripts
2. A target host fetches the script and executes it
3. The script calls back to pike with hostname, package type, architecture, and distro info
4. Pike matches the best sensor (local or via API) and responds with filename + SHA256
5. The script downloads the sensor, verifies the checksum, installs it, and reports back

## HTTP endpoints

| Path | Method | Description |
|---|---|---|
| `/lin` | GET | Linux bash install script |
| `/win` | GET | Windows PowerShell install script |
| `/s/{filename}` | GET | Sensor binary download |
| `/cb` | POST | Host callback (registration + sensor matching) |
| `/done` | POST | Installation result report |

All paths are optionally prefixed with `/<token>` when auth is enabled.

## Sensor matching

RPM sensors are matched by distro tag and architecture from the filename (e.g. `.el9.x86_64.rpm`). RHEL-family distros (Fedora, Rocky, Alma, Oracle, CentOS) use a fallback chain when an exact match is unavailable. DEB sensors are matched by architecture only since the same binary works across Ubuntu and Debian. Windows uses a single multi-arch installer.

## Project structure

```
src/
  main.rs           CLI entry point
  types.rs          Core types (AppState, Sensor, HostEntry)
  sensor_match.rs   Distro/arch matching logic
  scripts.rs        Install script generation
  falcon_api.rs     CrowdStrike API client
  util.rs           Sensor loading, token generation
  server/           HTTP server (axum)
  gui/              GUI (egui/eframe)
templates/
  linux.sh          Bash install template
  windows.ps1       PowerShell install template
```

## Tests

```
cargo test
```

## License

[MIT](LICENSE)
