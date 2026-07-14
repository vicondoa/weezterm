# Isolated Wayland title-bar UX harness

This harness starts a private compositor stack:

1. a fresh mode-`0700` `XDG_RUNTIME_DIR`;
2. headless Weston at `1400x900`;
3. nested Niri (including 26.04) using its automatically selected winit backend;
4. source-built WeezTerm with Wayland client-side decorations.

It never connects to the logged-in Wayland/X11 desktop, desktop D-Bus, Niri IPC,
or lock screen. Niri does not draw window titles. The title visible in each
captured PNG is WezTerm's client-side title bar and is therefore the UX evidence;
Niri window metadata is not used as proof.

The harness creates a private `wt-title.*` directory under the caller's user
runtime directory. Set `WEEZTERM_TITLE_RUNTIME_PARENT` to another short,
user-writable runtime parent in CI; the resulting path must remain short enough
for Unix domain sockets.

Weston's fixed `1400x900` headless output and kiosk shell deterministically size
the nested Niri output to the same dimensions. The minimal Niri fixture disables
animations and defines no startup applications or title-based window rules.

## Run

Build the source binaries first:

```console
cargo build -p wezterm -p wezterm-gui
```

Use the pinned Linux tools and source-binary runtime libraries exposed by the
root flake:

```console
nix develop .#wayland-title --command ./tests/wayland-title/run.sh --smoke
nix develop .#wayland-title --command ./tests/wayland-title/run.sh \
  --target tools.host.d2b
```

`nix run .#niri-title-test -- --smoke` is also available for compositor-only
validation. Use the development shell for a live source build so the untracked
Cargo binaries remain directly accessible.

The live d2b run requires the target to be disposable and reachable through
`/run/d2b/public.sock`. Override all relevant inputs when needed:

```console
nix develop .#wayland-title --command ./tests/wayland-title/run.sh \
  --target tools.host.d2b \
  --domain title-test \
  --shell-one title-a-123 \
  --shell-two title-b-123 \
  --d2b-socket /run/d2b/public.sock \
  --weezterm-bin "$PWD/target/debug/weezterm" \
  --weezterm-gui-bin "$PWD/target/debug/weezterm-gui" \
  --artifact-dir "$PWD/tests/wayland-title/results/manual"
```

Shell names are validated against WezTerm's safe 1-64 byte ASCII form. Defaults
include the harness PID to avoid reusing another run's persistent d2b sessions.
The harness closes its attachments but intentionally does not kill persistent
d2b shells; use caller-supplied disposable names if the daemon retains sessions.

Targets that intentionally permit only one simultaneous attachment can use
`--second-local`. The first tab remains a real process-bound d2b pane and proves
the trusted suffix. The second tab is an isolated local fixture used to prove
background OSC retention and immediate active-tab title switching without
forcing or evicting the d2b attachment:

```console
nix develop .#wayland-title --command ./tests/wayland-title/run.sh \
  --target tools.host.d2b --second-local
```

Without Nix, provide `weston`, `niri`, `grim`, `jq`, ImageMagick (`magick` and
`identify`), Tesseract, and standard GNU utilities on `PATH`.

## What the full run proves

The generated Lua config sets `enable_wayland = true` and
`window_decorations = 'TITLE | RESIZE'`. A `gui-startup` callback uses
`wezterm.mux.spawn_window` and `window:spawn_tab` with the configured d2b domain
and `WEEZTERM_D2B_SHELL_NAME`, producing two tabs in one window. Both
`WEEZTERM_D2B_BOUND_TARGET` and its compatibility alias
`WEEZTERM_D2B_BOUND_VM` bind the process to the caller's target.

The script discovers the sole private `gui-sock-d2b-*`, pins every
`weezterm cli` call to it, and uses the repository-verified commands
`list --format json`, `send-text --pane-id`, and
`activate-tab --tab-id`. It sets deterministic OSC 0 titles, updates the
background tab, and captures this sequence without desktop input automation:

- first tab active with its latest title;
- updated second tab immediately after activation;
- first tab again;
- second tab again.
- a long untrusted background title activated with the trailing trusted d2b
  identity still visible.
- an OSC 2 title update replacing an earlier OSC 1 icon title, matching
  applications such as Copilot CLI that update the window title independently.

Each state has a full screenshot, an upscaled title crop, raw/normalized OCR,
and the expected visible `title [target:shell]`. OCR failure fails the run.
Generated PNGs are compressed and checked to remain below 5 MB.

## Artifacts and failures

The default artifact directory is `tests/wayland-title/results/run-*`; `results/`
and private runtime directories are gitignored. Logs are retained for Weston,
Niri, WezTerm, CLI calls, validation, and Tesseract. Socket and process waits are
bounded. Errors point to the partial artifact directory, while the cleanup trap
terminates only recorded PIDs and removes only the harness-owned runtime.

Use `--smoke` when no disposable d2b target is available. It validates the Niri
fixture, private Weston/Niri sockets, compositor liveness, and `grim` capture.
Common failures are a missing source build, inaccessible d2b socket/target,
retained shell-name collision, software-rendering support, or OCR failing to
recognize the client-side title crop.
