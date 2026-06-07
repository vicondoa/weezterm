# ADR 0004: WeezTerm rebrand and binary compatibility

- **Status:** Accepted
- **Date:** 2026-04-06
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PR #2 (and #32 branded-config follow-up); [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md), [ADR 0008](0008-cicd-unification.md)

## Context

The fork ships as a distinct product, "WeezTerm", so it can be installed
alongside upstream WezTerm without clobbering its binaries, config, or desktop
integration. But upstream's packaging scripts, RPM/DEB `%install` sections, and
asset paths all reference the `wezterm` names. A rename that touched all of them
would conflict badly with every upstream merge.

## Decision

Rebrand the user-facing surface while keeping the rename out of upstream-owned
packaging logic:

- **Binaries.** The `[[bin]]` `name` fields produce `weezterm`, `weezterm-gui`,
  and `weezterm-mux-server` (Cargo *package* names are left as upstream's
  `wezterm*` to minimize churn).
- **App identity.** The desktop application id is `com.vicondoa.weezterm`, with
  branded `assets/weezterm.desktop` / `assets/weezterm.appdata.xml` and a
  branded icon set under `assets/icon/weezterm/`. Branding strings are
  centralized in `config/src/branding.rs`.
- **Packaging compatibility.** Rather than rewrite upstream's install scripts,
  `ci/deploy.sh` creates `wezterm* → weezterm*` compat symlinks (binaries,
  `.pdb`, icons) and copies the branded desktop/appdata files to the upstream
  names at package time, inside an [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md)
  sentinel block. The rest of `deploy.sh` then runs unmodified.

## Consequences

- **Positive:** WeezTerm coexists with WezTerm; packaging stays close to
  upstream, so `ci/deploy.sh` merges with minimal conflict (the only recurring
  one is the small branded block).
- **Negative / trade-offs:** the compat-symlink layer is indirection that must be
  kept in sync if upstream restructures packaging; package and binary names
  differ from Cargo package names, which can surprise newcomers.
- **Follow-ups:** how long the `wezterm*` compat aliases are retained is an open
  policy question to revisit if/when downstream packaging no longer needs them.
