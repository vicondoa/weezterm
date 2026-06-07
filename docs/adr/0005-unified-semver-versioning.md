# ADR 0005: Unified SemVer versioning

- **Status:** Accepted
- **Date:** 2026-04-08
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PR #18; [ADR 0008](0008-cicd-unification.md)

## Context

Upstream WezTerm versions by date-stamped nightly tags. The fork wants
predictable [SemVer](https://semver.org/) releases with a single, unambiguous
source of truth, and dev builds that are clearly distinguishable from releases
while still being traceable to a commit.

## Decision

- **Single source of truth.** `wezterm-version/Cargo.toml`'s `version` field is
  the canonical WeezTerm version (e.g. `0.2.0`). It is wrapped in an
  [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md) sentinel block.
- **Release builds** use the bare `X.Y.Z` from that field, written to a `.tag`
  file by the release workflow.
- **Dev builds** append `-dev.YYYYMMDD.HASH`, auto-derived from git
  (e.g. `0.2.0-dev.20260607.07acbf09`).
- Version info is plumbed at runtime through `config/src/version.rs`
  (`assign_version_info` / `wezterm_version`).

The release process (bump → commit → annotated `vX.Y.Z` tag → CI release) is
documented in [AGENTS.md → Release process](../../AGENTS.md#release-process).

## Consequences

- **Positive:** one field to bump; release vs. dev builds are visually
  distinct and every dev build maps back to a commit.
- **Negative / trade-offs:** diverges from upstream's nightly-date scheme, so
  version comparisons against upstream aren't meaningful.
- **Follow-ups:** none; the scheme is stable.

## Alternatives considered

- *Keep upstream's date-tag scheme* — rejected: no SemVer ordering and no clean
  single source for the fork's release cadence.
