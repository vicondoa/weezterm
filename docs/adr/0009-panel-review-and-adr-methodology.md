# ADR 0009: Panel review and ADR methodology

- **Status:** Accepted
- **Date:** 2026-06-07
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** [`vicondoa/nixling`](https://github.com/vicondoa/nixling) methodology; [AGENTS.md → Panel review](../../AGENTS.md#panel-review)

## Context

WeezTerm does plan-driven, multi-phase work (often agent-assisted) that touches
security boundaries, the mux protocol, and platform-specific rendering. Green
tests and CI are necessary but not sufficient for that class of change — the
sibling project [nixling](https://github.com/vicondoa/nixling) has the canonical
data point that an early review panel returned 0/8 sign-offs with 11 HIGH
findings that its static test gate caught *none* of. WeezTerm adopts nixling's
review methodology so the same rigor applies here.

## Decision

1. **Panel sign-off gate.** Multi-phase plans pass a panel gate at each phase
   boundary: *plan review → implementation → integration → work review →
   advance*. A phase closes only on unanimous (N/N) sign-off, where each reviewer
   returns a JSON record `{engineer, signoff, summary, recommendations}` and, by
   policy, `signoff == true` iff `recommendations == []`. The default reviewer
   roster and escape hatches (trivial fixes, time-critical hotfixes,
   docs-only changes) are specified in
   [AGENTS.md → Panel review](../../AGENTS.md#panel-review).
2. **ADRs.** Load-bearing fork decisions are recorded as ADRs under
   [`docs/adr/`](README.md), using [`TEMPLATE.md`](TEMPLATE.md). ADRs are short
   and immutable once accepted; a decision is changed by superseding ADR, not by
   editing.
3. **Hybrid commit convention.** Commits keep conventional prefixes
   (`feat:`/`fix:`/`chore:`), may add an optional panel-finding trailing tag
   (e.g. `( W1fu1 H3 )`) on panel-fix rounds, and add an `ADR:` body trailer when
   they implement or change an ADR-governed decision. Details in
   [AGENTS.md → Commit conventions](../../AGENTS.md#commit-conventions).
4. **Validation split.** Local validation in non-Windows dev environments is
   explicitly partial: lockfile consistency (`cargo metadata --locked`),
   dependency-light crate checks/tests (e.g. `wezterm-escape-parser`), and
   conflict-marker scans run locally; `cargo +nightly fmt`, full `cargo nextest
   run`, `ci/build-cross.sh` (Windows), and the `tests/ux/` harness are deferred
   to CI, which is the authoritative gate. PRs state what was/ wasn't validated
   locally.

## Consequences

- **Positive:** catches design-level defects that tests miss; durable decisions
  are discoverable; reviewers can trust the local-vs-CI boundary.
- **Negative / trade-offs:** the panel gate adds latency to multi-phase work
  (deliberately — a panel that catches one HIGH finding is cheaper than redoing
  integration); contributors must learn the conventions.
- **Follow-ups:** panel tooling is host-local and not a repo dependency;
  alternative implementations are fine as long as they preserve the review
  contract.
