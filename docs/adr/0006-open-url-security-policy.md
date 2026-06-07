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
  before any remote-initiated open. The other variants gate stricter/looser
  behavior explicitly.
- **Allow-list.** `OpenUrlConfig.allow_list` (seeded by
  `default_open_url_allow_list`) names URL prefixes that may open *without*
  confirmation. Anything not matched falls back to the default policy.
- **Scoping.** Policy is configurable globally (`config.open_url`) and
  overridable **per SSH domain**; `check_open_url_policy()` resolves the
  effective decision for a given URL and domain.

The decision is enforced at the point of opening (`wezterm-open-url`), so a
remote can *request* but never *force* a browser launch.

## Consequences

- **Positive:** safe-by-default — no silent remote-driven browser launches; power
  users can allow-list trusted prefixes; per-domain scoping limits blast radius.
- **Negative / trade-offs:** an extra confirmation prompt in the default flow;
  the allow-list must be curated to stay both safe and convenient.
- **Follow-ups:** revisit default allow-list entries as new remote-open use
  cases appear.

## Alternatives considered

- *Open all remote-initiated URLs silently* — rejected: unacceptable trust
  posture for a feature driven by remote hosts.
- *Block remote opens entirely* — rejected: defeats a core remote-extension use
  case; `Confirm` + allow-list gives the same safety with usable ergonomics.
