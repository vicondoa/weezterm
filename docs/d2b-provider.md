# Native d2b provider

WeezTerm includes a Linux-only native d2b provider for attaching terminal panes
to persistent shells on canonical d2b workload targets. The provider speaks the
d2b public daemon protocol through `d2b-client` and `d2b-toolkit-core`; it does
not connect to the privileged broker socket and does not shell out through the
`d2b` CLI for the terminal byte stream.

## Input alignment

Pin WeezTerm and d2b to one `nixpkgs` revision:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b = {
      url = "github:vicondoa/d2b";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    weezterm = {
      url = "github:vicondoa/weezterm/v0.7.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
}
```

WeezTerm pins toolkit release `v0.2.0` at the `v0.2.0` release tag in Cargo.
`Cargo.lock` resolves that tag to
`fde6af8b842718e7150f5056d4eba73093d4ad77`.

## Configuration

Use `target` for new d2b domains. A canonical target has the form
`<workload>.<realm>.d2b`; a legacy VM name remains valid during migration.

```lua
local wezterm = require 'wezterm'
local act = wezterm.action
local config = wezterm.config_builder()

config.d2b_domains = {
  {
    name = 'host-tools',
    target = 'tools.host.d2b',
  },
  {
    name = 'work',
    target = 'corp-vm',
  },
}

config.keys = {
  {
    key = 'D',
    mods = 'CTRL|SHIFT',
    action = act.ShowD2bLauncher,
  },
  {
    key = 'B',
    mods = 'CTRL|SHIFT',
    action = act.D2bOpenSession {
      domain = 'host-tools',
      name = 'build',
    },
  },
}

return config
```

The former `vm` field remains a compatibility alias through the 0.7 release
line. It normalizes into `target`; specifying both with different values is a
configuration error. `WEEZTERM_D2B_BOUND_VM` follows the same rule as an alias
for `WEEZTERM_D2B_BOUND_TARGET`. These aliases never select SSH, a host-terminal
backend, or another provider.

Canonical targets may contain dots. WeezTerm keeps the validated target as
metadata but derives bounded SHA-256-based keys for mux socket and generated
domain names, so a target is never used as a filesystem path component.

## Runtime contract

- Default socket: `/run/d2b/public.sock`.
- Transport: non-abstract Unix socket, bounded public-daemon frames, typed hello
  negotiation, and shell operations over the public socket.
- Discovery resolves compatibility VM aliases to the workload's canonical
  target and consumes toolkit provider, isolation, availability, and capability
  metadata.
- Attach uses target-aware shell operations, then forwards terminal input,
  stdout-only PTY output, resize, close-stdin, and close-attach operations.
- Pane close, pane kill, domain detach, and object drop close only the current
  attachment. They do not kill the persistent shell session.
- Backpressure, output gaps, stale sessions, daemon disconnects, and timeouts
  mark the pane as requiring reattach instead of pretending the stream is still
  healthy.
- Unsafe-local workloads are labeled `UNSAFE LOCAL — NO ISOLATION`. They require
  the negotiated `unsafe-local-shell-v1` feature before any shell operation.
  Helper and user-manager unavailability are shown in the domain and launcher
  UI. There is no SSH, direct host shell, or separate host-terminal fallback.
- Window-title d2b identity comes from the mux pane's configured d2b domain,
  never terminal-controlled user variables. Compatibility user variables are
  presentation metadata and cannot make a local or SSH pane appear d2b-backed.
- d2b pane and tab titles append that trusted identity as
  `application title [target:shell]`. OSC title changes remain dynamic. The
  most recent OSC 0, OSC 1, or OSC 2 title wins, so an OSC 2 update is not
  masked by an older icon title. The native window title follows the active
  tab's latest title, including updates received while the tab was in the
  background, and changes immediately when the active tab changes. When space
  is constrained, WeezTerm truncates the untrusted application portion from the
  right or left edge as appropriate while preserving the trailing trusted
  identity. Wayland client-side decorations scale configured title fonts down
  to fit the compact header with desktop-standard vertical padding.
- The tab-bar new-tab dropdown is target-scoped when its active pane belongs to
  a process-bound d2b domain. It offers one immediate generated-shell action and
  reattachment actions only for detached shells on that target. It never offers
  attached or unavailable sessions, manual shell naming, local/unmanaged
  shells, SSH/mux domains, or socket attachment. Non-d2b windows keep the
  generic new-tab dropdown.
- A persistent shell has one attachment owner. WeezTerm does not force attach:
  attached shells are omitted because another attachment would fail or require
  evicting the current owner. The sole owner supplies the initial terminal size
  and sends subsequent resize updates.

## Redaction and diagnostics

Targets, shell names, opaque session handles, terminal bytes, argv, environment
values, cwd, and raw socket paths are not metric labels. Diagnostics use bounded
operation names and correlation digests so an operator can correlate
reattach-required messages without exposing terminal content.

## Relationship to d2b-wlterm

`d2b-wlterm` remains the lightweight Waybar/Home Manager launcher for choosing
targets and shell names. WeezTerm is the terminal implementation: when launched
by `d2b-wlterm`, or configured directly by the operator, the native provider
uses the same public d2b shell protocol and toolkit crates.
