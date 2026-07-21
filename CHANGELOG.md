# Changelog

All notable changes to WeezTerm are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Migrated d2b configuration to the canonical v2 `TargetInput` workload type
  re-exported by `d2b-client-toolkit` 2.0.0. Workload domains now retain the
  canonical shell-service target without defining a local service DTO.
- Pinned the toolkit facade to the frozen daemon, guest, terminal, and user
  service client inventory while keeping endpoint acquisition and integrated
  workload routing disabled.
- Aligned the d2b client distribution name and packaged source layout with
  `share/d2b-client-toolkit/{distribution,d2b}`.
- Advanced the pinned `d2b-client-toolkit` distribution revision to
  `926de54e7320599c373524a10b65aaf13b6ff422`. The canonical d2b source
  revision and fingerprint the facade re-exports are unchanged.
- Made `nix develop --command make precommit` hermetic: the pinned Nix dev
  shell now provides `cargo-nextest` directly, and `make fmt` falls back to
  bare `cargo fmt` (which resolves to that same pinned nightly rustfmt) when
  no `rustup` is present instead of failing on `cargo +nightly fmt`.

### Removed

- Removed the legacy public-socket hello, JSON shell DTO, seqpacket framing,
  target-alias, session-picker, and terminal-bridge integration. Native d2b
  discovery and persistent-shell attachment remain unavailable until canonical
  workload-to-shell routing is integrated.

## [0.7.2] - 2026-07-13

### Fixed

- Kept Wayland client-side title text within the compact header height and
  honored the most recent OSC 1/OSC 2 title update for native d2b panes.

## [0.7.1] - 2026-07-13

### Added

- A reproducible Linux Wayland title-bar harness using an isolated headless
  Weston parent and dedicated nested Niri compositor.

### Fixed

- d2b window titles now follow the active tab's latest OSC or explicit title
  immediately when switching tabs and append the trusted `[target:shell]`
  identity as a suffix.
- Wayland client-side title bars now render the current window title instead of
  showing a blank frame when the compositor does not draw server-side titles.
- The new-tab dropdown in a d2b-bound window now offers only immediate shell
  creation and detached shells from the current target; regular windows retain
  the generic launcher.
- Allowed native d2b shell attachment enough time to complete the daemon's
  guest-control health path instead of aborting functional VM shells at the
  generic two-second socket I/O deadline, while keeping discovery and teardown
  fail-fast.

## [0.7.0] - 2026-07-11

### Added

- Native Linux d2b provider core using the public client protocol with
  non-blocking pane queues and redacted diagnostics.
- Provider-neutral d2b target domains and a session picker with named-shell
  prompts, per-target mux isolation, canonical target titles, and typed
  provider posture.
- Canonical dotted workload targets with bounded, non-reversible mux socket and
  generated domain keys.
- Clear unsafe-local/no-isolation, helper-unavailable, and daemon feature-skew
  status in d2b domain and launcher UI.
- Native d2b provider documentation covering toolkit alignment, Lua
  configuration, migration, runtime behavior, and redaction boundaries.

### Changed

- The native d2b provider now uses canonical `target` identities across config,
  discovery, transport, panes, reconnect state, launcher entries, and user vars.
- d2b public client dependencies and Nix inputs are pinned to d2b-toolkit
  `v0.2.0`.
- Native d2b shell panes now read from d2b's PTY stdout stream only, matching
  the persistent-shell wire contract and avoiding false reattach prompts when a
  separate stderr stream is unavailable.
- Source-built Nix packages now include Wayland runtime libraries in their
  binary rpaths, and Wayland-only launches fail with the original Wayland error
  instead of falling back to X11 and hiding the missing library cause.
- Native d2b provider connections now use d2bd's SOCK_SEQPACKET public socket
  transport, preserving frame packet boundaries for shell attach/read/write
  requests.
- Native d2b socket I/O is reactor-backed and non-blocking, so stalled daemon
  traffic cannot block the mux executor; packet buffers are reused and
  interrupted reads and writes retry. Interrupted and backlog-saturated
  connects await bounded reactor completion.
- Cargo manifests use the immutable d2b-toolkit `v0.2.0` release tag; the Nix
  flakes avoid a duplicate input that did not participate in the Cargo build.
- Nix source builds and development shells pin exact Rust releases, and
  non-Linux shell evaluation no longer instantiates Wayland-only libraries.

### Deprecated

- The d2b domain `vm` field and `WEEZTERM_D2B_BOUND_VM` remain compatibility
  aliases for `target` and `WEEZTERM_D2B_BOUND_TARGET` through the 0.7 release
  line.

### Security

- Unsafe-local shell targets require negotiated `unsafe-local-shell-v1`
  support and never fall back to SSH, a direct host shell, or a separate
  host-terminal backend.
- Conflicting target and VM aliases fail closed before domain selection.
- Window-title d2b identity now comes from the trusted mux domain rather than
  terminal-controlled user variables, preventing local or SSH panes from
  spoofing a d2b-backed title.

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
