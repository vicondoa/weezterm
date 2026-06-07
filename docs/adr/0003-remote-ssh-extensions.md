# ADR 0003: Remote SSH extensions

- **Status:** Accepted
- **Date:** 2026-04-08
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PRs #1, #17; design doc [`docs/remote-extensions.md`](../remote-extensions.md); [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md), [ADR 0006](0006-open-url-security-policy.md)

## Context

WeezTerm's reason for existing is a better *remote* (SSH/mux) experience than
upstream WezTerm provides: discovering services listening on a remote host,
forwarding their ports back to the client, and letting remote programs open
URLs in the client's browser. These need to cross the multiplexer protocol and
must be added without destabilizing upstream's local-terminal behavior.

## Decision

The remote feature set is implemented as fork-only modules plus additive mux
protocol changes:

- **Port detection** — `mux/src/port_detect.rs` discovers listening ports on the
  remote (including a terminal-output scanner adapter).
- **Port forwarding** — `mux/src/port_forward.rs` (state manager) and
  `mux/src/port_forward_proxy.rs` (TCP proxy), surfaced through the
  `wezterm-gui/src/overlay/port_forward.rs` overlay UI, with smart conflict
  handling for already-bound local ports. SSH direct-tcpip channels carry the
  forwarded traffic.
- **Remote browser open** — an `OSC 7457` escape sequence
  (parsed in `wezterm-escape-parser`) lets a remote program request a URL open,
  and `$BROWSER` is injected into remote SSH shells so CLI tools route through
  the client. URL opening is gated by [ADR 0006](0006-open-url-security-policy.md).
- **Transport** — new PDU types are appended to the `codec/` `pdu!` macro for
  port forwarding (`GetDetectedPorts`, `RequestPortForward`, `StopPortForward`,
  …) and remote URL opening, and dispatched on both the server
  (`wezterm-mux-server-impl`) and client (`wezterm-client`). The remote-URL PDU
  path is live; the **mux-server-side port-forward handlers are currently stubs**
  (they return empty/`success: false` with "not yet implemented in mux server
  mode"). Active port forwarding today runs through the local
  `PortForwardManager`/proxy against SSH domains directly, not via the mux
  server.

Every touch to an upstream-owned file follows
[ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md). The full design,
configuration, and usage are documented in
[`docs/remote-extensions.md`](../remote-extensions.md).

## Consequences

- **Positive:** the headline fork capabilities are isolated in new files and
  additive PDUs, so they survive upstream merges and are easy to audit.
- **Negative / trade-offs:** new PDUs expand the mux protocol surface and must
  preserve `CODEC_VERSION` compatibility; SSH backends (`ssh2`, `libssh-rs`)
  must both support the channel work.
- **Follow-ups:** implement the mux-server-side port-forward PDU handlers
  (`GetDetectedPorts` / `RequestPortForward` / `StopPortForward`) so forwarding
  works in mux-server mode, not just against direct SSH domains; remote URL
  opening's trust boundary is owned by
  [ADR 0006](0006-open-url-security-policy.md).
