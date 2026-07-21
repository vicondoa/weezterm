# Architecture decision records

Architecture decision records (ADRs) capture the load-bearing design choices
that make WeezTerm a *fork* of [WezTerm](https://github.com/wezterm/wezterm)
rather than a drop-in build of it. They explain **why** the fork diverges from
upstream where it does, so that future contributors (and upstream-merge work)
can tell intentional divergence from accidental drift.

ADRs complement, rather than replace, the longer design docs under `docs/`
(e.g. [`remote-extensions.md`](../remote-extensions.md) and
[`windows-rendering-design.md`](../windows-rendering-design.md)) and the
operating manual in [`AGENTS.md`](../../AGENTS.md). An ADR is short: the
context, the decision, and its consequences. When a topic has a full design
doc, the ADR records the decision and links out rather than duplicating it.

## When to write an ADR

Write (or update) an ADR when a change:

- diverges from upstream WezTerm in a way that must survive future merges
  (new subsystems, renamed binaries, changed defaults);
- establishes or changes a fork-wide policy (security posture, versioning,
  CI/CD shape, review process);
- makes a decision that a reviewer would reasonably ask "why was it done this
  way?" six months later.

Trivial, reversible, or purely-internal changes do **not** need an ADR.

## How to write one

1. Copy [`TEMPLATE.md`](TEMPLATE.md) to `NNNN-short-title.md`, where `NNNN` is
   the next free zero-padded number.
2. Fill in context / decision / consequences. Keep each ADR factual and
   present-tense — it records a decision that is *in effect*, not a proposal to
   debate (unless `Status: Proposed`).
3. Add a row to the index below.
4. ADRs are immutable once `Accepted`. To change a decision, write a new ADR and
   mark the old one `Superseded by [ADR XXXX]`.
5. Commits that implement or change an ADR-governed decision carry an `ADR:`
   trailer in the commit body (see
   [AGENTS.md → Commit conventions](../../AGENTS.md#commit-conventions)).

## Index

| ADR | Status | Date | Summary |
| --- | --- | --- | --- |
| [0001. Fork strategy and clean-merge discipline](0001-fork-strategy-and-clean-merge-discipline.md) | Accepted | 2026-06-07 | Every fork change is additive and wrapped in `// --- weezterm remote features ---` sentinels; prefer new files; never reformat or rename upstream code. |
| [0002. Upstream sync policy](0002-upstream-sync-policy.md) | Accepted | 2026-06-07 | Sync from `upstream/main` via true merge commits (merge-base must advance), even though feature PRs are squash-merged; resolve `Cargo.lock`/`ci/deploy.sh` narrowly. |
| [0003. Remote SSH extensions](0003-remote-ssh-extensions.md) | Accepted | 2026-04-08 | Remote port detection + forwarding, a TCP proxy, OSC 7457 remote-browser open, and `$BROWSER` injection; carried over new mux PDUs (server-side port-forward handlers still stubbed) and fork-only modules. |
| [0004. WeezTerm rebrand and binary compatibility](0004-weezterm-rebrand-and-binary-compat.md) | Accepted | 2026-04-06 | Output binaries are `weezterm*`, the app id is `com.vicondoa.weezterm`; `ci/deploy.sh` creates `wezterm*` compat symlinks so packaging keeps working. |
| [0005. Unified SemVer versioning](0005-unified-semver-versioning.md) | Accepted | 2026-04-08 | `wezterm-version/Cargo.toml` is the single version source; releases use the bare `X.Y.Z`, dev builds append `-dev.YYYYMMDD.HASH`. |
| [0006. open-url security policy](0006-open-url-security-policy.md) | Accepted | 2026-04-09 | Remote-initiated URL opens default to `Confirm`, gated by an allow-list and enforced at the client/frontend call sites; non-`http(s)` schemes always denied. (Per-domain override is defined but not yet wired.) |
| [0007. Windows rendering modes and window-state persistence](0007-windows-rendering-modes-and-state-persistence.md) | Accepted | 2026-05-24 | Three render modes (WgpuDComp / WgpuClassic / SoftwareRdp); Auto uses WgpuDComp, falling back to WgpuClassic for RDP/virtual-GPU (SoftwareRdp is opt-in pending a fix). Plus window-state persistence, validated by `tests/ux/`. |
| [0008. CI/CD unification](0008-cicd-unification.md) | Accepted | 2026-04-06 | A single `weezterm_build.yml` builds/tests/releases all platforms; the upstream `gen_*.yml` workflows are kept byte-identical but disabled, to merge cleanly. |
| [0009. Panel review and ADR methodology](0009-panel-review-and-adr-methodology.md) | Accepted | 2026-06-07 | Adopt nixling's panel sign-off gate for multi-phase work, this ADR framework, a hybrid commit convention, and an explicit local-vs-CI validation split. |
| [0010. Provider-neutral d2b target domains](0010-provider-neutral-d2b-target-domains.md) | Superseded by d2b ADR 0045 | 2026-07-11 | Historical d2b 1.x public-socket domain design; the current seam stores canonical v2 client targets and defers runtime integration to finalized service contracts. |
