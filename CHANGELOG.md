# Changelog

All notable changes to WeezTerm are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

WeezTerm is a fork of [WezTerm](https://github.com/wezterm/wezterm).
Versions use semver; each release notes which upstream wezterm snapshot
it is based on.

## [Unreleased]

## [0.6.0] - 2026-06-19

### Changed

- Version scheme: semver (previously date-based)
- Pre-built binary delivery via fetchurl + autoPatchelfHook in flake
- Based on upstream wezterm/main at 2026-06-07 (8afe0ad30)

### Added

- Changelog-driven nix release workflow
- Changelog validation CI gate for PRs
- GitHub Pages binary cache

## [0.5.1] - 2026-06-19

### Fixed

- CI: serve pre-built binaries via GitHub Pages Nix binary cache

## [0.5.0] - 2026-06-19

### Added

- Changelog-driven nix release workflow (auto-publishes pre-built binaries)
- Changelog validation CI gate for PRs

## [0.4.0] - 2026-04-07

### Added

- Auto-install weezterm binaries on remote hosts for SSH multiplexing
- `remote_install_binaries_dir` config for cross-platform dev

## [0.3.0] - 2026-04-06

### Changed

- Rebrand from WezTerm to WeezTerm
- Nix flake outputs use `weezterm` binary name exclusively
