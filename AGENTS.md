# AGENTS.md — Guide for AI Coding Agents

This is **WeezTerm**, a fork of [WezTerm](https://github.com/wezterm/wezterm) with
remote SSH extensions. This document contains essential information for AI agents
working on this codebase.

## Quick Reference

| Task | Command |
|------|---------|
| **Pre-commit (run before PR)** | **`make precommit`** |
| **Cross-build (Windows + Linux)** | **`ci/build-cross.sh`** |
| Build | `cargo build -p wezterm -p wezterm-gui -p wezterm-mux-server` |
| Check (fast) | `cargo check` |
| Check specific crate | `cargo check -p <crate>` |
| Test all | `cargo nextest run` |
| Test specific crate | `cargo nextest run -p <crate>` |
| Test escape parser (no_std) | `cargo nextest run -p wezterm-escape-parser` |
| **UX tests (Windows)** | **`cd tests/ux && pip install -r requirements.txt && python -m pytest -v -s`** |
| Format | `cargo +nightly fmt` |
| Lint | `cargo clippy` |

## Project Structure

WeezTerm is a Cargo workspace with 16 members. Key crates:

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

## Pre-commit Workflow

**Always run `make precommit` before pushing or creating a PR.** This runs the same
checks that CI will enforce:

1. `cargo +nightly fmt` — format all code (nightly required)
2. `cargo check` — compile-check the full workspace
3. `cargo nextest run` + escape-parser no_std tests

On Windows, ensure Strawberry Perl is in your PATH for the OpenSSL build:
```bash
export PATH="/c/Strawberry/perl/bin:$HOME/.cargo/bin:$PATH"
make precommit
```

If `make` is not available, run the steps manually:
```bash
cargo +nightly fmt
cargo check
cargo nextest run
cargo nextest run -p wezterm-escape-parser
```

### Cross-Build Verification

**Always run `ci/build-cross.sh` to verify both Windows and Linux builds succeed.**
This script builds Windows binaries natively and Linux binaries via WSL, then
assembles a ready-to-test package in `target/cross-pkg/`. This catches issues
that `cargo check` alone misses (e.g., platform-specific compilation, rustc
ICEs on Linux, and linker errors).

```bash
# From Git Bash on Windows:
ci/build-cross.sh              # debug build
ci/build-cross.sh --release    # release build
```

Output:
```
target/cross-pkg/
├── windows/          Windows binaries (weezterm.exe, weezterm-gui.exe, …)
└── linux-x86_64/     Linux binaries  (weezterm, weezterm-mux-server)
```

**Important**: The Linux/WSL build uses a separate Rust toolchain and can
surface warnings/errors that don't appear on Windows (e.g., unused code
warnings that trigger rustc ICEs on certain Linux compiler versions). Always
verify both platforms compile cleanly.

## WeezTerm Remote Features

All WeezTerm-specific additions (as opposed to upstream WezTerm code) are marked with:
```rust
// --- weezterm remote features ---
```

This makes merge conflicts with upstream easy to identify and resolve.

### New files (fork-only, no merge risk):
- `mux/src/port_detect.rs` — Remote port detection
- `mux/src/port_forward.rs` — Port forwarding state manager
- `mux/src/port_forward_proxy.rs` — TCP proxy
- `wezterm-gui/src/overlay/port_forward.rs` — Port manager overlay UI
- `docs/remote-extensions.md` — Remote features documentation

### Additive changes to existing files:
Changes to upstream files are small, additive-only (new enum variants, match arms, methods),
and always delimited with the `// --- weezterm remote features ---` comment.

## Adding Code That Merges Cleanly with Upstream

WeezTerm is a fork of WezTerm. All fork-specific code must be structured for easy merging.
Follow these rules strictly:

### Rule 1: Mark every change with begin AND end sentinel comments
Every block of WeezTerm-specific code in an upstream file **must** be wrapped
with both a begin and end sentinel. This is mandatory for **all** multi-line
additions — no exceptions:
```rust
// --- weezterm remote features ---
fn my_new_function() {
    // ...
}
// --- end weezterm remote features ---
```

For **single-line** additions only (e.g. one new enum variant, one match arm,
or one `mod` statement), a single begin comment above the line is sufficient —
no end comment needed:
```rust
// --- weezterm remote features ---
MyNewVariant,
```

**Checklist before committing changes to upstream files:**
- [ ] Every multi-line block has `// --- weezterm remote features ---` before it
- [ ] Every multi-line block has `// --- end weezterm remote features ---` after it
- [ ] Single-line additions have at least the begin comment
- [ ] Comments use the exact strings above (for `grep` searchability)
- [ ] In non-Rust files (Makefile, YAML), use the appropriate comment syntax:
      `# --- weezterm remote features ---` / `# --- end weezterm remote features ---`

### Rule 2: Prefer new files over modifying upstream files
- New modules go in new files → zero merge conflicts.
- Register them from existing files with a small, marked `mod` statement.
- Example: `mux/src/port_detect.rs` is a new file; `mux/src/lib.rs` has a one-line
  `// --- weezterm remote features ---\npub mod port_detect;` addition.

### Rule 3: Additive-only changes to upstream files
- **Add** enum variants, match arms, methods, trait impls — never delete or rename upstream code.
- Place new enum variants at the **end** of the enum.
- Place new match arms at the **end**, before any wildcard (`_`) arm.
- Keep additions as small and self-contained as possible.

### Rule 4: Use feature gating where practical
If a change is large, consider gating it behind a cargo feature flag:
```toml
# Cargo.toml
[features]
remote-extensions = []
```
```rust
#[cfg(feature = "remote-extensions")]
mod port_detect;
```
This lets upstream compile without the fork code entirely.

### Rule 5: Do not touch formatting or refactor upstream code
- Never reformat upstream files (even with `cargo +nightly fmt` if it changes upstream lines).
- Never rename upstream symbols.
- Never move upstream code between files.

### Rule 6: Keep Cargo.toml changes minimal
- Add new dependencies at the **end** of `[dependencies]`.
- New workspace members go at the **end** of `members = [...]`.
- Never modify existing dependency versions.

### Rule 7: New Makefile targets
Add WeezTerm-specific Makefile targets at the **end** of the file, after a
`# --- weezterm remote features ---` comment. Never modify existing targets.

### Merge workflow
```bash
git remote add upstream https://github.com/wezterm/wezterm.git
git fetch upstream
git merge upstream/main          # or rebase, per preference
# Search for conflict markers, resolve by keeping both sides:
#   upstream code stays as-is, WeezTerm additions stay in sentinel blocks
```

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
| Window resize/DPI handling | `wezterm-gui/src/termwindow/resize.rs`, `window/src/os/windows/window.rs` |
| Window state persistence | `wezterm-gui/src/window_state_persistence.rs` |
| UX tests (automated) | `tests/ux/` (see UX Testing section below) |
| UX tests (manual) | `tests/ux/MANUAL_TESTS.md` |

## UX Testing

WeezTerm has a Python-based UX test harness at `tests/ux/` that launches the
real `weezterm-gui.exe` binary, manipulates windows via Win32 API, captures
screenshots, and asserts on behavior. **Run these tests after any changes to
window management, resize, DPI handling, or startup code.**

### Automated Tests

```bash
# Prerequisites: build the binary first
cargo build -p wezterm-gui

# Install Python dependencies (once)
cd tests/ux
pip install -r requirements.txt

# Run all UX tests
python -m pytest -v -s

# Run specific suite
python -m pytest test_resize.py -v -s       # resize behavior
python -m pytest test_maximize.py -v -s      # maximize/unmaximize
python -m pytest test_dimensions.py -v -s    # state persistence across restarts
python -m pytest test_startup.py -v -s       # startup time and rendering
```

The tests are **fully isolated** from any running WeezTerm instances via:
- `--config-file <temp>` prevents connecting to existing GUI instances
- `XDG_CONFIG_HOME=<temp>` isolates config dirs and `window-state.json`
- `XDG_RUNTIME_DIR=<temp>` isolates sockets and pid files

Test suites:
- `test_startup.py` — startup time threshold, window fully drawn after launch
- `test_resize.py` — shrink/grow without artifacts, rapid resize, extreme sizes
- `test_maximize.py` — maximize/restore preserves dimensions, no oversized window
- `test_dimensions.py` — window size/position/maximized state persisted across restarts
- `test_ssh_mux.py` — SSH mux connection startup, resize, and maximize over SSH mux
  (connects to `jvicondo-a7` with an isolated workspace; requires SSH access)

Failed tests save screenshots to `tests/ux/test-results/` for debugging.

### Manual Tests

Some UX scenarios require manual testing because they depend on hardware
configurations that can't be automated (e.g., multiple monitors with different
DPI scaling).

**See `tests/ux/MANUAL_TESTS.md`** for the full checklist. Key scenarios:

- **M1–M2:** Cross-monitor drag between monitors with different DPI — verify the
  window matches the drag outline and doesn't balloon
- **M3:** Drag outline vs final window position — verify they match
- **M4:** Maximize on one monitor, drag to another
- **M5:** Rapid cross-monitor bouncing — verify no crash or size drift

**When to run manual tests:** After any changes to:
- `window/src/os/windows/window.rs` (window event handling, DPI)
- `wezterm-gui/src/termwindow/resize.rs` (resize/scaling logic)
- `wezterm-gui/src/window_state_persistence.rs` (state save/restore)

### Known Issues (tracked in `tests/ux/FINDINGS.md`)

1. **Window position saved as (0,0)** — `save_current_window_state()` in
   `termwindow/mod.rs:2015-2023` never populates x/y coordinates
2. **Oversized window after maximize→close→reopen→restore** — saves maximized
   dimensions instead of normal (restored) dimensions from WINDOWPLACEMENT
3. **Missing `WM_DPICHANGED` handler** — `window/src/os/windows/window.rs`
   does not handle `WM_DPICHANGED`, causing window to balloon when dragged
   between monitors with different DPI instead of using the Windows-suggested rect
4. **`connect --workspace` crashes SSH mux** — using `--workspace` flag with `connect`
   subcommand causes the SSH mux connection to drop after ~6-8s with PDU decode EOF.
   Without `--workspace`, connections are stable. Root cause is in spawn_tab_in_domain_if_mux_is_empty.
5. **Content stretching during resize** — terminal content is visually stretched
   during window resize before being redrawn at the correct dimensions. Multiple
   intermediate redraws create a jarring experience.

## CI/CD Pipelines

### Active workflows (WeezTerm fork)

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| **weezterm-build** | `.github/workflows/weezterm_build.yml` | push (main, feature/\*, ci/\*), PR to main, `v*` tags | **Primary CI/CD**: builds Windows + macOS + Linux matrix, runs tests, packages artifacts, creates GitHub Releases on tags |
| fmt | `.github/workflows/fmt.yml` | push, PR | Checks `cargo +nightly fmt` formatting |
| termwiz | `.github/workflows/termwiz.yml` | push, PR | Tests the termwiz library |
| wezterm-ssh | `.github/workflows/wezterm_ssh.yml` | push, PR | Tests SSH features |
| Nix | `.github/workflows/nix.yml` | push, PR | Nix build check |
| Lock Threads | `.github/workflows/lock.yml` | scheduled | Auto-locks old issues |
| No Response | `.github/workflows/no-response.yml` | scheduled | Auto-closes unresponsive issues |
| Dependabot Updates | (dynamic) | scheduled | Dependency security PRs |

### Disabled workflows (upstream, kept for merge compatibility)

All `gen_*.yml` workflows (33 files) are **disabled via the GitHub Actions API**.
They are upstream WezTerm per-platform build workflows that our unified
`weezterm_build.yml` replaces. The files are **kept identical to upstream** so
that `git merge upstream/main` produces no conflicts.

**Do NOT modify `gen_*.yml` files.** If an upstream merge updates them, accept
the upstream changes as-is. They will remain disabled.

Also disabled: `nix_continuous.yml`, `nix-update-flake.yml`, `pages.yml`,
`verify-pages.yml`.

### Release process

1. Bump `version` in `wezterm-version/Cargo.toml` (e.g., `0.2.0` → `0.3.0`)
2. Commit: `git commit -am "chore: bump version to 0.3.0"`
3. Tag: `git tag -a v0.3.0 -m "WeezTerm v0.3.0"  &&  git push origin v0.3.0`
4. The `weezterm-build` workflow triggers on `v*` tags
5. Writes `.tag` file with the version (e.g., `0.3.0`)
6. Builds all platforms, runs tests, packages artifacts
7. `release` job creates a GitHub Release titled `WeezTerm v0.3.0`

**Version format:**
- Release builds: `0.3.0` (from `.tag` file)
- Dev builds: `0.3.0-dev.YYYYMMDD.SHORTHASH` (auto-derived from git)
- The single source of truth is `wezterm-version/Cargo.toml`

### Branch protection

- `main` requires the `windows` status check to pass before merge
- Squash-merge only (no merge commits, no rebase)
- Auto-delete branches after merge
