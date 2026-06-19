# Changelog

All notable changes to WeezTerm are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-04-07

### Added

- Auto-install weezterm binaries on remote hosts for SSH multiplexing
- `remote_install_binaries_dir` config for cross-platform dev (Windows → Linux)
- `ci/build-cross.sh` for building Windows + Linux via WSL
- Mux-only tarballs in CI releases for lightweight remote deployment

### Fixed

- Fix tilde expansion in remote path commands
- Replace SFTP upload with reliable SSH exec+stdin
- Resolve merge conflict markers in CI workflow

## [0.3.0] - 2026-04-06

### Changed

- Rebrand from WezTerm to WeezTerm (casing, GitHub URLs, desktop entries)
- Nix flake outputs use `weezterm` binary name exclusively

### Fixed

- Dependency bumps (time, rustls-webpki, bytes, tar)
