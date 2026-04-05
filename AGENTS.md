# AGENTS.md — Guide for AI Coding Agents

This is **Weezterm**, a fork of [WezTerm](https://github.com/wezterm/wezterm) with
remote SSH extensions. This document contains essential information for AI agents
working on this codebase.

## Quick Reference

| Task | Command |
|------|---------|
| Build | `cargo build -p wezterm -p wezterm-gui -p wezterm-mux-server` |
| Check (fast) | `cargo check` |
| Check specific crate | `cargo check -p <crate>` |
| Test all | `cargo nextest run` |
| Test specific crate | `cargo nextest run -p <crate>` |
| Test escape parser (no_std) | `cargo nextest run -p wezterm-escape-parser` |
| Format | `cargo +nightly fmt` |
| Lint | `cargo clippy` |

## Project Structure

Weezterm is a Cargo workspace with 16 members. Key crates:

### Binaries
- `wezterm/` — CLI entrypoint
- `wezterm-gui/` — GUI terminal emulator (main application)
- `wezterm-mux-server/` — Multiplexer server daemon

### Core Libraries
- `term/` — Terminal model and escape sequence processing (NOT a workspace member — used via path dep)
- `termwiz/` — Terminal primitives, input handling, surface rendering (NOT a workspace member)
- `wezterm-escape-parser/` — Escape sequence parser (**supports no_std!** see below)
- `wezterm-surface/` — Surface/cell model, hyperlink detection
- `codec/` — Client↔server mux protocol (binary framed, serde + leb128 + varbincode + zstd)
- `mux/` — Multiplexer: domains, panes, tabs, SSH integration
- `config/` — Configuration parsing, Lua bindings
- `pty/` — Pseudo-terminal abstraction (cross-platform)

### SSH
- `wezterm-ssh/` — SSH client library (supports both `ssh2` and `libssh-rs` backends)
- `wezterm-client/` — Client-side mux connection logic

### Utilities
- `wezterm-open-url/` — Opens URLs in the system browser
- `wezterm-cell/` — Cell/glyph types
- `wezterm-dynamic/` — Dynamic typing for Lua bridge

## Architecture Patterns

### Error Handling
- **`anyhow`** for application-level errors and error context (`.context("...")`)
- **`thiserror`** for library error types (`#[derive(thiserror::Error)]`)
- `wezterm-escape-parser` has custom `bail!`/`ensure!` macros in `src/error.rs`

### Async Runtime
- **`smol`** is the async runtime (NOT tokio). Use `smol::channel`, `smol::spawn`, `smol::block_on`
- `async-trait` for async trait methods
- `filedescriptor` crate for cross-platform fd/socket handling

### Logging
- Use the **`log`** crate (`log::info!`, `log::debug!`, `log::warn!`, `log::error!`)
- NOT `tracing` — this codebase uses `log` + `env_logger`

### Serialization
- **`serde`** with `Serialize`/`Deserialize` derives for config and protocol types
- The mux protocol (`codec/`) uses a custom binary format: leb128 length framing + varbincode + optional zstd compression
- Config structs use `#[serde(default)]` extensively

### Testing
- Use `#[cfg(test)] mod test { ... }` for unit tests
- `k9::snapshot!` for snapshot testing (used in `term/`, `mux/`, `wezterm-gui/`)
- `TestTerm` helper in `term/src/test/mod.rs` for terminal behavior tests
- SSH integration tests use real `sshd` via `wezterm-ssh/tests/sshd.rs` fixture
- `rstest` + `assert_fs` for SSH E2E tests

## Critical Gotchas

### no_std: wezterm-escape-parser
`wezterm-escape-parser` compiles as **no_std by default**. When adding code to this crate:
- Do NOT use `std::` imports without gating on `#[cfg(feature = "std")]`
- Use `alloc::` for `String`, `Vec`, `Box` etc. when not in `std` mode
- The Makefile runs it separately: `cargo nextest run -p wezterm-escape-parser`

### Formatting requires nightly
Run `cargo +nightly fmt`, not `cargo fmt`. There is no `rustfmt.toml`.

### Cargo patches
`Cargo.toml` patches `cairo-sys-rs` to a local path (`deps/cairo`). Don't remove this.

### .cargo/config.toml
Windows builds have special linker and `crt-static` settings. Don't modify unless you know what you're doing.

### SSH backend feature flags
`wezterm-ssh` has two optional backends: `ssh2` and `libssh-rs` (both enabled by default).
When adding SSH features, implement for BOTH backends using the pattern:
```rust
match self {
    #[cfg(feature = "ssh2")]
    Self::Ssh2(sess) => { /* ssh2 impl */ }
    #[cfg(feature = "libssh-rs")]
    Self::LibSsh(sess) => { /* libssh impl */ }
}
```

### Codec version
When adding new PDU types to `codec/src/lib.rs`:
- Append entries at the END of the `pdu!` macro
- Bump `CODEC_VERSION` if changes are backwards-incompatible
- Each PDU type needs a unique numeric ID

## Weezterm Remote Features

All Weezterm-specific additions (as opposed to upstream WezTerm code) are marked with:
```rust
// --- weezterm remote features ---
```

This makes merge conflicts with upstream easy to identify and resolve.

### New files (fork-only, no merge risk):
- `mux/src/port_detect.rs` — Remote port detection
- `mux/src/port_forward.rs` — Port forwarding state manager
- `wezterm-gui/src/overlay/port_forward.rs` — Port manager overlay UI
- `wezterm-client/src/port_forward_proxy.rs` — TCP proxy
- `term/src/test/remote_features.rs` — Tests for remote features
- `wezterm-ssh/tests/e2e/direct_tcpip.rs` — SSH forwarding tests
- `docs/remote-extensions.md` — Remote features documentation

### Additive changes to existing files:
Changes to upstream files are small, additive-only (new enum variants, match arms, methods),
and always delimited with the `// --- weezterm remote features ---` comment.

## Key File Locations for Common Tasks

| Task | Files |
|------|-------|
| Add new escape sequence | `wezterm-escape-parser/src/osc.rs` (or `csi.rs`) |
| Handle escape in terminal | `term/src/terminalstate/performer.rs` |
| Add terminal alert type | `term/src/terminal.rs` (`Alert` enum) |
| Handle alert in GUI | `wezterm-gui/src/frontend.rs` |
| Add mux protocol message | `codec/src/lib.rs` (`pdu!` macro) |
| Handle message on server | `wezterm-mux-server-impl/src/sessionhandler.rs` |
| Handle message on client | `wezterm-client/src/client.rs` |
| Add SSH session capability | `wezterm-ssh/src/session.rs`, `sessioninner.rs`, `sessionwrap.rs` |
| Add keybinding/command | `config/src/keyassignment.rs`, `wezterm-gui/src/commands.rs` |
| Add overlay/picker UI | `wezterm-gui/src/overlay/` (follow `launcher.rs` pattern) |
| Add config option | `config/src/ssh.rs` (for SSH), `config/src/lib.rs` (for global) |
| Spawn env vars | `mux/src/domain.rs` (local), `mux/src/ssh.rs` (remote SSH) |
