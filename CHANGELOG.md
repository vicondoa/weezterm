# Changelog

All notable changes to WeezTerm are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Documented native d2b provider packaging, flake input alignment with
  d2b-toolkit, runtime behavior, and redaction boundaries.
- Native Linux d2b provider core using the d2b public client protocol with non-blocking pane queues and redacted diagnostics.
- Native d2b VM terminal domains and a d2b session picker with offline-state handling, named-shell prompts, per-VM mux isolation, and VM/session-aware titles.

### Changed

- Source-built Nix packages now include Wayland runtime libraries in their
  binary rpaths, and Wayland-only launches fail with the original Wayland error
  instead of falling back to X11 and hiding the missing library cause.
- Nix flake packaging declares `d2b-toolkit` as an input and rewrites native d2b
  client crate paths during builds, avoiding developer-local absolute paths.

## [0.6.0] - 2026-06-19

### Changed

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
