# ADR 0010: Provider-neutral d2b target domains

- **Status:** Accepted
- **Date:** 2026-07-11
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** d2b
  [ADR 0044](https://github.com/vicondoa/d2b/blob/main/docs/adr/0044-unsafe-local-runtime-provider.md);
  [ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md);
  [ADR 0005](0005-unified-semver-versioning.md)

## Context

The first native d2b domain treated every shell endpoint as a local VM name.
d2b now exposes provider-neutral canonical workload targets and typed provider,
isolation, availability, and capability metadata. Some targets intentionally use
the `unsafe-local` provider, which is convenient but is not an isolation
boundary.

WeezTerm must support these targets without creating a second host-terminal or
SSH transport, leaking target strings into filesystem names, or breaking
existing VM configurations during the migration window.

## Decision

- `target` is the authoritative d2b domain identity. Dotted canonical targets
  are normalized through d2b-toolkit's public target type.
- The `vm` config field and `WEEZTERM_D2B_BOUND_VM` remain aliases through at
  least the 0.7 release line. If an alias and its canonical field differ,
  configuration or startup fails before selecting a domain.
- Legacy VM names continue over the same d2b public-socket shell protocol. An
  alias never selects SSH, a direct host shell, or another backend.
- Workload discovery supplies the canonical reconnect identity and typed
  provider, isolation, availability, and persistent-shell capability posture.
- Unsafe-local shells are visibly labeled as having no isolation and require
  negotiated `unsafe-local-shell-v1` support. Missing helpers or feature skew
  fail visibly without fallback.
- Mux socket and generated domain keys use a bounded, domain-separated SHA-256
  digest. The validated target remains metadata and is never a filesystem path
  component.
- Unix public-socket integration remains Linux-gated so Windows and macOS keep
  compiling without unconditional Unix APIs.

## Consequences

- Canonical targets, including unsafe-local workloads, share one terminal
  transport and retain stdout-only PTY behavior.
- Existing VM configurations continue to work while receiving a migration path
  to `target`.
- Operators see explicit no-isolation and helper-unavailable posture in domain
  and launcher UI.
- Ephemeral mux socket names change to opaque target keys and cannot be inferred
  from the target.
- The fork carries a small additive config, mux, and launcher surface that must
  be preserved during upstream merges.
