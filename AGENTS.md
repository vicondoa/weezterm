# AGENTS.md — Guide for AI Coding Agents

This is **WeezTerm**, a fork of [WezTerm](https://github.com/wezterm/wezterm) with
remote SSH extensions. This document contains essential information for AI agents
working on this codebase.

## Quick Reference

| Task | Command |
|------|---------|
| **Build & test (run before PR)** | **`ci/build-cross.sh`** (Git Bash) |
| Format | `cargo +nightly fmt` |
| Check (fast, single platform) | `cargo check` |
| Check specific crate | `cargo check -p <crate>` |
| Test all | `cargo nextest run` |
| Test specific crate | `cargo nextest run -p <crate>` |
| Test escape parser (no_std) | `cargo nextest run -p wezterm-escape-parser` |
| **UX tests (Windows)** | **`cd tests/ux && pip install -r requirements.txt && python -m pytest -v -s`** |
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

**Always run `ci/build-cross.sh` from Git Bash before pushing or creating a PR.**
This is the single command that validates your changes. It builds Windows
binaries natively and Linux binaries via WSL, runs tests, and assembles a
ready-to-test package. It catches platform-specific issues that `cargo check`
alone misses.

```bash
# From Git Bash on Windows (ensure Strawberry Perl is in PATH):
export PATH="/c/Strawberry/perl/bin:$HOME/.cargo/bin:$PATH"
ci/build-cross.sh              # debug build (faster)
ci/build-cross.sh --release    # release build
```

Output:
```
target/cross-pkg/
├── windows/          Windows binaries (weezterm.exe, weezterm-gui.exe, …)
└── linux-x86_64/     Linux binaries  (weezterm, weezterm-mux-server)
```

### What CI checks (and you should too)

The following checks run in CI on every PR to `main`. All must pass before merge.

| CI Workflow | What it does | Local equivalent |
|-------------|-------------|------------------|
| **check-code-formatting** (`fmt.yml`) | `cargo +nightly fmt --all -- --check` | `cargo +nightly fmt` |
| **weezterm-build / windows** (required gate) | Full build + `cargo nextest run` on Windows | `ci/build-cross.sh` (Windows part) |
| **weezterm-build / macos** | Build ARM64 + x86_64, run tests | macOS only (CI covers this) |
| **weezterm-build / linux** ×8 distros | Build + test in Docker containers | `ci/build-cross.sh` (Linux/WSL part) |
| **Nix** (`nix.yml`) | `nix build .` | If you changed Rust files, `.github/workflows/nix.yml`, root `flake.nix`, `flake.lock`, or `nix/**` |
| **termwiz** (`termwiz.yml`) | `cargo build/test -p termwiz --all-features` | Only if you changed `termwiz/**` |
| **wezterm-ssh** (`wezterm_ssh.yml`) | Build + test SSH crate | Only if you changed `wezterm-ssh/**` |
| **CodeQL** | Security analysis (actions + rust) | N/A (CI only) |

The **`windows`** job is the required status check for merge. The other jobs
are informational but should also pass.

### If `make` is available

`make precommit` runs format + check + tests but does NOT cross-build for Linux:
```bash
cargo +nightly fmt
cargo check
cargo nextest run
cargo nextest run -p wezterm-escape-parser
```

Prefer `ci/build-cross.sh` which covers both platforms.

**Important**: The Linux/WSL build uses a separate Rust toolchain and can
surface warnings/errors that don't appear on Windows (e.g., unused code
warnings that trigger rustc ICEs on certain Linux compiler versions). Always
verify both platforms compile cleanly.

## Panel review

WeezTerm uses the same panel-review contract as `vicondoa/nixling` for
plan-driven or multi-phase work. Green tests and CI are necessary but not
sufficient to advance or merge that kind of work: the relevant phase must receive
unanimous panel sign-off.

For each phase:

1. **Plan review** — panel reviews the plan; iterate until every selected
   reviewer returns `signoff: true`.
2. **Implementation** — perform the work.
3. **Integration** — merge or consolidate the implementation output.
4. **Work review** — panel reviews the integrated diff; iterate on findings until
   every selected reviewer returns `signoff: true`.
5. **Advance/merge** — only then may the next phase begin or the PR merge.

A phase closes only on **unanimous (N/N)** sign-off. Each reviewer returns a
JSON record:

```json
{
  "engineer": "security",
  "signoff": true,
  "summary": "What was reviewed and the overall posture.",
  "recommendations": []
}
```

By policy, `signoff` is `true` **iff** `recommendations` is `[]`. If any
reviewer returns findings, land the fixes, rerun tests, and start another panel
round. Green tests do not waive this gate. See
[ADR 0009](docs/adr/0009-panel-review-and-adr-methodology.md) for the binding
methodology decision.

Use the nixling default panel roster unless a plan explicitly narrows the panel:

| Engineer | Focus |
|----------|-------|
| `software` | Shell, Nix/package shape, idempotency, and error handling. |
| `test` | Coverage of new behavior, invisible regressions, and validation gaps. |
| `nixos` | NixOS/Home Manager module wiring, `lib.mkForce` / `lib.mkDefault`, activation ordering, and host integration. |
| `networking` | SSH/mux/network surface changes, forwarding behavior, firewall posture, and routing assumptions. |
| `security` | Attack surface, trust boundaries, capability/secret handling, URL/open-browser safety, and telemetry/audit hygiene. |
| `rust` | Rust API shape, error propagation, unsafe/FFI boundaries, dependency direction, and testability. |
| `product` | Operator UX, naming, migration/deprecation policy, defaults, and actionable errors. |
| `docs` | Docs accuracy, Diataxis fit where relevant, release notes, and AGENTS.md updates landing with load-bearing behavior changes. |
| `observability` | Log/metric/span shape, cardinality, retention/defaults, and diagnostic usefulness without leaking secrets. |
| `kernel` | PTY, process, signal, filesystem, windowing, GPU/driver, and Linux/Windows/macOS API edge cases. |

Escape hatches match nixling: trivial fixes may skip the panel; time-critical
hotfixes may skip the pre-fix panel but need a post-fix panel; documentation-only
changes may skip unless they describe load-bearing behavior. Autopilot mode does
not waive the panel gate.

## Architecture Decision Records (ADRs)

Load-bearing fork decisions are recorded as ADRs under
[`docs/adr/`](docs/adr/README.md). An ADR explains **why** the fork diverges
from upstream WezTerm where it does, so future contributors (and upstream-merge
work) can tell intentional divergence from accidental drift.

- **When to write one:** a change that diverges from upstream in a way that must
  survive merges, establishes/changes a fork-wide policy, or that a reviewer
  would later ask "why was it done this way?". Trivial, reversible, or
  purely-internal changes don't need one.
- **How:** copy [`docs/adr/TEMPLATE.md`](docs/adr/TEMPLATE.md) to
  `NNNN-short-title.md`, fill in context / decision / consequences, and add a row
  to the [index](docs/adr/README.md). ADRs are immutable once `Accepted`; change
  a decision by writing a new ADR that supersedes the old one.
- **Current ADRs** cover: clean-merge discipline (0001), upstream-sync policy
  (0002), remote SSH extensions (0003), the rebrand + binary compat (0004),
  unified SemVer versioning (0005), the open-url security policy (0006), Windows
  rendering modes + state persistence (0007), CI/CD unification (0008), and this
  panel-review + ADR methodology (0009).

## Commit conventions

WeezTerm uses a **hybrid** convention:

- **Subject.** Short, imperative, with a conventional-commit prefix naming the
  area: `feat:`, `fix:`, `chore:`, `docs:`, `build:`, `ci:`, `refactor:`.
- **Body.** Wrap at ~72 cols; explain *why*, not what.
- **Panel-finding tag (optional).** Commits produced in a panel-fix round may end
  the subject with a parenthesized nixling-style tag, e.g. `( W1fu1 H3 )` —
  wave 1, follow-up round 1, addressing HIGH-3 (`C`/`H`/`M`/`L` = finding
  severity). Use this only for panel-fix rounds; everyday commits don't need it.
- **`ADR:` trailer (scoped).** A commit that implements or changes an
  ADR-governed decision carries an `ADR:` trailer line in the body, e.g.
  `ADR: 0007` (comma-separate multiples: `ADR: 0007, 0009`). This keeps the
  subject tag clean while staying `grep`-able. Only commits touching an
  ADR-governed decision need it — not every commit.
- **Co-author trailer.** Keep the `Co-authored-by: Copilot <…>` trailer on
  agent-assisted commits.

Example:

```
fix: clamp restored window to the target monitor work area ( W2fu1 H1 )

WINDOWPLACEMENT restore could place the window off-screen when the
saved monitor is gone. Clamp to the nearest monitor's work area.

ADR: 0007
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

**Merge policy.** Feature PRs to `main` are **squash-merge only**. The one
exception is an upstream sync, which lands as a **true merge commit** so the
merge-base advances; see
[ADR 0002](docs/adr/0002-upstream-sync-policy.md).

## WeezTerm Remote Features

All WeezTerm-specific additions (as opposed to upstream WezTerm code) are marked with:
```rust
// --- weezterm remote features ---
```

This makes merge conflicts with upstream easy to identify and resolve.

> Decision record: [ADR 0003 — Remote SSH extensions](docs/adr/0003-remote-ssh-extensions.md);
> full design in [`docs/remote-extensions.md`](docs/remote-extensions.md).

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

> Decision records: [ADR 0001 — Fork strategy and clean-merge discipline](docs/adr/0001-fork-strategy-and-clean-merge-discipline.md)
> and [ADR 0002 — Upstream sync policy](docs/adr/0002-upstream-sync-policy.md).

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

> Decision record: [ADR 0007 — Windows rendering modes and window-state persistence](docs/adr/0007-windows-rendering-modes-and-state-persistence.md).

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

`tests/ux/FINDINGS.md` is the source of truth for current status. As of this
writing the only open item is:

1. **`connect --workspace` crashes SSH mux** — using `--workspace` with a
   non-default workspace on the `connect` subcommand drops the SSH mux
   connection after ~6–8s with a PDU decode EOF. Without `--workspace`,
   connections are stable. Suspected root cause in
   `spawn_tab_in_domain_if_mux_is_empty()` (`wezterm-gui/src/main.rs`). The UX
   tests work around it by sharing the default workspace.

Previously-tracked issues now **resolved** (see `FINDINGS.md` for the fixing
commits): window position saved as (0,0); oversized window after
maximize→close→reopen→restore; content stretching during resize; and the
missing `WM_DPICHANGED` handler (fixed, but still wants manual verification on a
multi-monitor/mixed-DPI setup).

## CI/CD Pipelines

> Decision record: [ADR 0008 — CI/CD unification](docs/adr/0008-cicd-unification.md).

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

All `gen_*.yml` workflows (21 files, after upstream dropped the EOL
debian11/fedora39/fedora40/ubuntu20.04 sets) are **disabled via the GitHub
Actions API**.
They are upstream WezTerm per-platform build workflows that our unified
`weezterm_build.yml` replaces. The files are **kept identical to upstream** so
that `git merge upstream/main` produces no conflicts.

**Do NOT modify `gen_*.yml` files.** If an upstream merge updates them, accept
the upstream changes as-is. They will remain disabled.

Also disabled: `nix_continuous.yml`, `nix-update-flake.yml`, `pages.yml`,
`verify-pages.yml`.

### Release process

> Decision record: [ADR 0005 — Unified SemVer versioning](docs/adr/0005-unified-semver-versioning.md).

Releases are **changelog-driven**. Merging to `main` with a new version
header in `CHANGELOG.md` triggers an automatic release.

**Workflow:**

1. During development, add entries under `## [Unreleased]` in `CHANGELOG.md`
2. When ready to release:
   - Bump `version` in `wezterm-version/Cargo.toml`
   - Rename `## [Unreleased]` entries to `## [X.Y.Z] - YYYY-MM-DD`
   - Add a fresh empty `## [Unreleased]` section above
3. Merge PR to `main`
4. The `nix_release.yml` workflow detects the new version, auto-tags `vX.Y.Z`,
   builds the Nix package, and publishes a GitHub Release with binaries

**The CI changelog gate requires:**
- Every PR that changes code must update CHANGELOG.md
- Format must follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
- Version headers: `## [X.Y.Z] - YYYY-MM-DD` (valid semver + ISO date)

**Version format:**
- Release builds: `0.4.0` (from CHANGELOG.md + Cargo.toml)
- Dev builds: `0.4.0-dev.YYYYMMDD.SHORTHASH` (auto-derived from git)
- Single source of truth: `CHANGELOG.md` (determines when a release is cut);
  `wezterm-version/Cargo.toml` provides the in-binary version string

**Nix release assets (x86_64-linux):**
- `weezterm-vX.Y.Z-x86_64-linux-nix.tar.gz` — full `nix build` output
  (binaries + terminfo + shell completions + desktop entry)
- `SHA256SUMS`

### Branch protection

- `main` requires the `windows` status check to pass before merge
- Squash-merge only (no merge commits, no rebase)
- Auto-delete branches after merge
