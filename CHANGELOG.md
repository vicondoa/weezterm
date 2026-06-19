# Changelog

All notable changes to WeezTerm are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions track the upstream wezterm snapshot date (`YYYYMMDD`) with a
`-nixling.N` patch suffix for our releases.

## [Unreleased]

## [20260607-nixling.1] - 2026-06-19

### Changed

- Version scheme: `YYYYMMDD-nixling.N` (upstream date + patch number)
- Pre-built binary delivery via fetchurl + autoPatchelfHook in flake
- Based on upstream wezterm/main at 2026-06-07 (8afe0ad30)

### Added

- Changelog-driven nix release workflow
- Changelog validation CI gate for PRs
- GitHub Pages binary cache
