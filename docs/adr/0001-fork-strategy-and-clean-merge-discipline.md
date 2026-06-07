# ADR 0001: Fork strategy and clean-merge discipline

- **Status:** Accepted
- **Date:** 2026-06-07
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** [ADR 0002](0002-upstream-sync-policy.md), [AGENTS.md → Adding Code That Merges Cleanly with Upstream](../../AGENTS.md)

## Context

WeezTerm is a long-lived fork of [WezTerm](https://github.com/wezterm/wezterm)
that continuously tracks upstream. Every line the fork changes in a file that
upstream also owns is a future merge conflict. Uncontrolled divergence would
make upstream merges expensive and would blur the line between intentional
fork behavior and accidental drift.

## Decision

Fork changes follow a strict clean-merge discipline:

1. **Sentinel comments.** Every multi-line block of fork-specific code in an
   upstream-owned file is wrapped in
   `// --- weezterm remote features ---` … `// --- end weezterm remote features ---`
   (use the language-appropriate comment syntax in non-Rust files). Single-line
   additions carry at least the begin sentinel. The exact strings are mandatory
   so they stay `grep`-able.
2. **Prefer new files.** New behavior lives in new modules (e.g.
   `mux/src/port_detect.rs`), registered from upstream files via a small,
   sentinel-marked `mod` line. New files never conflict.
3. **Additive only.** Add enum variants, match arms, methods, and trait impls at
   the *end* of the relevant construct (before any wildcard arm). Never delete,
   rename, move, or reformat upstream code.
4. **Minimal manifest churn.** New dependencies and workspace members go at the
   end of their lists; existing upstream dependency versions are never edited as
   part of a fork change.

The authoritative, checklist-level rules live in
[AGENTS.md](../../AGENTS.md#adding-code-that-merges-cleanly-with-upstream).

## Consequences

- **Positive:** upstream merges are usually conflict-free; conflicts that do
  occur are localized and obvious. Fork code is auditable by grepping the
  sentinel string.
- **Negative / trade-offs:** the fork sometimes accepts a less elegant additive
  shape over a cleaner refactor; contributors must learn the sentinel
  convention.
- **Follow-ups:** large divergences may be feature-gated behind a cargo feature
  so upstream can compile without the fork code at all.

## Alternatives considered

- *Rebasing the fork onto upstream each cycle* — rejected: rewrites published
  history and re-resolves the same conflicts every time.
- *Vendoring upstream and patching* — rejected: loses upstream git history and
  the advancing merge-base (see [ADR 0002](0002-upstream-sync-policy.md)).
