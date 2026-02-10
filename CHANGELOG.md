# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Sensor grouping tags support (`--tags`) — comma-separated tags passed to
  `falconctl --tags` on Linux and `GROUPING_TAGS=` on Windows during installation.
- Default `deployment/pike` tag automatically applied to all deployments, making
  it easy to identify hosts installed via Pike in the Falcon console.
- `--no-default-tag` CLI flag to opt out of the automatic `deployment/pike` tag.
- GUI: new "Sensor" configuration card with a tags input field.
- GUI: "Default tag" checkbox in Advanced settings to toggle the automatic tag.
