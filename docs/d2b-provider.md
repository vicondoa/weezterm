# Native d2b provider

WeezTerm includes a Linux-only native d2b provider for attaching terminal panes
to d2b persistent guest shells. The provider speaks the d2b public daemon
protocol through `d2b-client` and `d2b-toolkit-core`; it does not connect to the
privileged broker socket and does not shell out through the `d2b` CLI for the
terminal byte stream.

## Flake input alignment

Pin WeezTerm, d2b, and the shared toolkit to one `nixpkgs` revision:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b = {
      url = "github:vicondoa/d2b";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    weezterm = {
      url = "github:vicondoa/weezterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-toolkit.follows = "d2b-toolkit";
    };
  };
}
```

The WeezTerm flake rewrites its local Cargo path dependencies to the toolkit
source selected by the flake lock during Nix builds, so the lock selects the
exact d2b public client protocol used by the binary.

## Runtime contract

- Default socket: `/run/d2b/public.sock`.
- Transport: non-abstract Unix socket, bounded public-daemon frames, typed hello
  negotiation, and shell operations over the public socket.
- Attach: creates or attaches a d2b persistent shell, then forwards terminal
  input, output, resize, close-stdin, and close-attach operations.
- Pane close, pane kill, domain detach, and object drop close only the current
  attachment. They do not kill the persistent shell session.
- Backpressure, output gaps, stale sessions, daemon disconnects, and timeouts
  mark the pane as requiring reattach instead of pretending the stream is still
  healthy.

## Redaction and diagnostics

Shell names, opaque session handles, terminal bytes, argv, environment values,
cwd, and raw socket paths are not written to broad debug output. Diagnostics use
bounded operation names and correlation digests so an operator can correlate
reattach-required messages without exposing guest terminal content.

## Relationship to d2b-wlterm

`d2b-wlterm` remains the lightweight Waybar/Home Manager launcher for choosing
VMs and shell names. WeezTerm is the terminal implementation: when launched by
`d2b-wlterm`, or configured directly by the operator, the native provider uses
the same public d2b shell protocol and toolkit crates.
