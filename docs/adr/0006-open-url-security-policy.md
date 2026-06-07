# ADR 0006: open-url security policy

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PR #19; [ADR 0003](0003-remote-ssh-extensions.md); `config/src/ssh.rs`, `wezterm-open-url/`

## Context

[ADR 0003](0003-remote-ssh-extensions.md) lets a *remote* host ask the *local*
client to open a URL (via `OSC 7457` and `$BROWSER` injection). That crosses a
trust boundary: a compromised or hostile remote could otherwise drive the
client into launching arbitrary URLs (phishing, `file://`/custom-scheme abuse,
local service hits) with no user awareness. The fork must not silently open
remote-initiated URLs.

## Decision

Remote-initiated URL opens are governed by an explicit, fail-safe policy in
`config/src/ssh.rs`:

- **`OpenUrlPolicy`** has a default of **`Confirm`** — the user is prompted
  (via a toast) before any remote-initiated open. `Allow` opens immediately and
  `Deny` silently blocks; non-`http(s)` schemes (`file://`, `javascript:`,
  `data:`, …) are **always** denied regardless of policy.
- **Allow-list.** `OpenUrlConfig.allow_list` (seeded by
  `default_open_url_allow_list`) names URL prefixes that may open *without*
  confirmation. Anything not matched falls back to the default policy.
- **Scoping.** The global policy lives at `config.open_url`. A per-SSH-domain
  override field (`SshDomain.open_url`) exists and `check_open_url_policy()`
  accepts a domain config, **but the current enforcement call sites pass `None`,
  so the effective policy today is the global one** — per-domain scoping is
  defined in config but not yet plumbed to the open sites (a known follow-up).

Enforcement happens at the **client/frontend call sites** that handle a
remote open request — `wezterm-client` (`client.rs`) and the GUI frontend
(`wezterm-gui/src/frontend.rs`) — which consult `check_open_url_policy()`
*before* invoking `wezterm-open-url` to actually launch the browser. So a remote
can *request* but never *force* a browser launch. (`wezterm-open-url` itself only
opens; it does not re-check policy, so new callers must gate at the call site.)

## Consequences

- **Positive:** safe-by-default — no silent remote-driven browser launches; power
  users can allow-list trusted prefixes; non-`http(s)` schemes are always blocked.
- **Negative / trade-offs:** an extra confirmation prompt in the default flow;
  the allow-list must be curated to stay both safe and convenient; policy is
  enforced per call site rather than centrally in `wezterm-open-url`, so a new
  caller could bypass it if it forgets the check.
- **Follow-ups:** plumb the originating SSH domain's `open_url` config into the
  enforcement call sites (currently they pass `None`, so per-domain overrides are
  inert); consider centralizing enforcement; revisit default allow-list entries
  as new remote-open use cases appear.

## Alternatives considered

- *Open all remote-initiated URLs silently* — rejected: unacceptable trust
  posture for a feature driven by remote hosts.
- *Block remote opens entirely* — rejected: defeats a core remote-extension use
  case; `Confirm` + allow-list gives the same safety with usable ergonomics.
