# ADR 0002: Upstream sync policy

- **Status:** Accepted
- **Date:** 2026-06-07
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md), [ADR 0008](0008-cicd-unification.md)

## Context

The fork must pull upstream WezTerm changes (bug fixes, security updates,
features) on an ongoing basis. Two constraints pull in opposite directions:

- For *feature PRs*, `main` is protected to **squash-merge only** so the fork's
  own history stays linear and readable.
- For *upstream syncs*, git needs the merge to be a real two-parent merge commit
  so the **merge-base advances**. If an upstream sync is squashed or rebased,
  git loses the record that `upstream/main` is an ancestor, and the *next* sync
  re-evaluates every upstream commit from scratch — re-introducing already
  resolved conflicts.

## Decision

Upstream is synced via a **true merge commit**:

1. Add the `upstream` remote (`https://github.com/wezterm/wezterm.git`), fetch,
   and `git merge --no-ff upstream/main` on a `feature/*` branch.
2. Resolve conflicts **narrowly**, keeping both sides where they are
   independent:
   - `ci/deploy.sh`: keep the fork's branded/binary-compat blocks *and*
     upstream's changes.
   - `Cargo.lock`: do not hand-merge. Take a single consistent side, then verify
     with `cargo metadata --locked`; only regenerate if it proves inconsistent,
     and inspect the diff. Never silently revert a fork dependency bump (e.g.
     `openssl`).
   - Upstream deletions of EOL `gen_*.yml` workflows are accepted as-is
     (see [ADR 0008](0008-cicd-unification.md)).
3. Land the sync PR with GitHub's **"Create a merge commit"**, *not* squash/
   rebase. Branch protection is temporarily relaxed to allow the merge commit
   and re-locked to squash-only afterward. Verify `main`'s tip is a merge commit
   whose parents are the previous fork `main` and an `upstream/main` ancestor.

## Consequences

- **Positive:** each subsequent upstream merge only has to consider commits
  since the last sync; conflicts don't recur.
- **Negative / trade-offs:** `main` carries occasional merge commits, breaking
  strict linearity; landing a sync requires a manual branch-protection toggle.
- **Follow-ups:** upstream's transitive `Cargo.lock` "cooldown" bumps are not
  pulled in by a sync that touches no manifests; refresh them in a dedicated
  `cargo update` PR when desired.

## Alternatives considered

- *Squash every upstream sync* — rejected: defeats merge-base advancement.
- *Cherry-pick upstream commits individually* — rejected: unscalable and also
  fails to advance the merge-base.
