# ADR 0008: CI/CD unification

- **Status:** Accepted
- **Date:** 2026-04-06
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PR #2; [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md), [ADR 0002](0002-upstream-sync-policy.md), [ADR 0004](0004-weezterm-rebrand-and-binary-compat.md)

## Context

Upstream WezTerm uses ~33 generated per-platform/per-distro workflows
(`gen_*.yml`, plus `nix_continuous.yml`, `pages.yml`, …). The fork wants one
coherent pipeline that builds, tests, and releases the WeezTerm-branded
artifacts across platforms — but deleting the upstream workflow files would
conflict with every upstream merge that regenerates them.

## Decision

- **One unified pipeline.** `.github/workflows/weezterm_build.yml` builds the
  Windows + macOS + Linux matrix, runs tests, packages branded artifacts, and
  creates GitHub Releases on `v*` tags. Supporting fork workflows: `fmt.yml`,
  `termwiz.yml`, `wezterm_ssh.yml`, `nix.yml`.
- **Upstream workflows kept but disabled.** The `gen_*.yml` workflows (and
  `nix_continuous.yml`, `nix-update-flake.yml`, `pages.yml`,
  `verify-pages.yml`) are **disabled via the GitHub Actions API**, and their
  files are kept **byte-identical to upstream**. They are not edited by the fork.
  When an upstream merge updates them, the upstream version is accepted as-is and
  they stay disabled. Upstream deletions of EOL `gen_*.yml` files are likewise
  accepted (see [ADR 0002](0002-upstream-sync-policy.md)).
- **Required gate.** The `windows` job in `weezterm_build.yml` is the required
  status check for merge to `main`.

## Consequences

- **Positive:** a single, branded, end-to-end pipeline; zero merge conflicts on
  the large upstream workflow surface; one required gate to reason about.
- **Negative / trade-offs:** disabled workflow files linger in the tree (clutter)
  and depend on out-of-band API state to stay disabled; the canonical Windows/UX
  validation only runs in CI, not in Linux dev environments (see
  [ADR 0009](0009-panel-review-and-adr-methodology.md)).
- **Follow-ups:** keep `weezterm_build.yml` as the source of truth; never modify
  `gen_*.yml`.

## Alternatives considered

- *Delete the upstream `gen_*.yml` files* — rejected: guarantees recurring merge
  conflicts, violating [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md).
