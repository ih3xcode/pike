# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-31

### Changed

- **BREAKING:** сервер переїхав під підкоманду `pike serve`. Стара форма
  `pike --sensor X --cid Y` більше не підтримується. GUI запускається без
  аргументів або через `pike gui`.
- **BREAKING:** маршрут завантаження змінився з `/s/{filename}` на `/s/{sha256}`.
- Сенсори з API більше не змішуються з локальними файлами — матчинг завжди
  йде по свіжому списку з API, тож версія сенсора більше не «застигає» на
  першій завантаженій.
- Локальний матчинг RPM більше не має fallback «будь-який пакет цієї
  архітектури», який міг віддати пакет чужої дистрибуції.
- Згенерований токен тепер 32 символи замість 8.
- Типовий таймаут тепер 0 (без обмеження) замість 30 хвилин.
- Невдала автентифікація на старті більше не веде беззастережно до виходу:
  три спроби, а далі, якщо CID відомий, сервер стартує на дисковому кеші.

### Added

- Конфіг-файл TOML (`--config`, типово `/etc/pike/pike.toml`) з пріоритетом
  флаги > env > конфіг > дефолти.
- Змінні середовища `PIKE_CLIENT_ID`, `PIKE_CLIENT_SECRET`, `PIKE_CID`,
  `PIKE_TOKEN` — секрети більше не обовʼязково передавати в argv.
- Стабільний токен (`--token` або `[server] token`) — ванлайнер переживає
  рестарт.
- Дисковий кеш сенсорів за sha256 з перевіркою цілісності, дедуплікацією
  паралельних завантажень і витісненням за розміром.
- `--public-url` для роботи за reverse proxy з TLS.

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
