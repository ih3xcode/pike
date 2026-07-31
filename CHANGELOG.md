# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-31

### Changed

- **BREAKING:** the server moved under the `pike serve` subcommand. The old
  `pike --sensor X --cid Y` form is no longer supported. The GUI starts with no
  arguments or via `pike gui`.
- **BREAKING:** the download route changed from `/s/{filename}` to `/s/{sha256}`.
- API sensors are no longer merged into the local sensor list — matching always
  runs against a fresh API listing, so the served sensor version no longer
  freezes on whatever was downloaded first.
- Local RPM matching dropped the "any package of this architecture" fallback,
  which could hand a host a package built for a different distribution.
- Generated tokens are now 32 characters instead of 8.
- The default timeout is now 0 (run forever) instead of 30 minutes.
- Startup authentication is retried three times with backoff; if it still
  fails, pike exits non-zero rather than starting up unable to serve anything.
  Under `Restart=always` systemd retries and the failure stays visible in
  `systemctl status`.
- `pike update --apply` now verifies the release asset's sha256 and refuses to
  update when the checksum is missing or does not match.
- CI publishes a `.sha256` file alongside every release binary.

### Added

- TOML config file (`--config`, `/etc/pike/pike.toml` by default) with
  flags > env > config > defaults precedence.
- `PIKE_CLIENT_ID`, `PIKE_CLIENT_SECRET`, `PIKE_CID` and `PIKE_TOKEN`
  environment variables — secrets no longer have to be passed in argv.
- Pinned token (`--token` or `[server] token`) so the one-liner survives a
  restart.
- Disk-backed sensor cache keyed by sha256, with integrity verification,
  deduplication of concurrent downloads and size-based eviction.
- `--public-url` for running behind a TLS reverse proxy.
- `pike service-install` — an interactive wizard that validates credentials
  against the API, then creates a system user, a 0640 config, a cache directory
  and a hardened systemd unit.
- `pike service-uninstall [--purge]`.
- Optional auto-update timer (off by default), kept separate from the service:
  under `ProtectSystem=strict` the service cannot rewrite its own binary.

## [0.3.0] - 2026-02-10

### Added

- Sensor grouping tags support (`--tags`) — comma-separated tags passed to
  `falconctl --tags` on Linux and `GROUPING_TAGS=` on Windows during installation.
- Default `deployment/pike` tag automatically applied to all deployments, making
  it easy to identify hosts installed via Pike in the Falcon console.
- `--no-default-tag` CLI flag to opt out of the automatic `deployment/pike` tag.
- GUI: new "Sensor" configuration card with a tags input field.
- GUI: "Default tag" checkbox in Advanced settings to toggle the automatic tag.

## [0.2.1] - 2026-02-09

### Changed

- Strict sensor matching — Pike no longer falls back to a random sensor when no
  exact match is found. Returns a clear error instead of silently serving an
  incompatible package.
- Install scripts now display the server's error message (e.g. "no matching
  sensor available") instead of opaque HTTP error codes.

### Added

- Filename validation warnings — when using local sensor files with
  unrecognizable filenames (missing distro/arch tags), Pike shows warnings in
  both CLI and GUI.

### Fixed

- US-1 cloud selection routing to the correct API endpoint.

## [0.2.0] - 2026-02-09

### Added

- Self-update system — `pike update` checks for new releases,
  `pike update --apply` downloads and replaces the binary in-place.
- Auto-update check — runs in background on startup; CLI shows a banner notice,
  GUI shows an update banner with one-click install.
- `pike --version` flag.
- Document required API scope (Sensor Download, Read) in README.
