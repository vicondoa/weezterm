# ADR 0007: Windows rendering modes and window-state persistence

- **Status:** Accepted
- **Date:** 2026-05-24
- **Deciders:** WeezTerm maintainers (@vicondoa)
- **Relates to:** PRs #23, #27, #28; design doc [`docs/windows-rendering-design.md`](../windows-rendering-design.md); UX harness `tests/ux/`

## Context

WeezTerm is used heavily on Windows, including over RDP and on virtual GPUs.
Upstream's wgpu/WebGpu front-end has no RDP detection and uses DX12 on virtual
adapters in RDP sessions, which forces expensive GPU readback and visible
artifacts. Separately, the window's size/position/maximized state was not
reliably restored across restarts, and content visibly stretched during resize.
These are platform-specific correctness and UX problems upstream does not solve.

## Decision

- **Three rendering modes**, auto-selected at startup based on RDP/GPU
  detection: **WgpuDComp**, **WgpuClassic**, and **SoftwareRdp**. The selection
  rationale, detection logic, and a phased implementation plan live in
  [`docs/windows-rendering-design.md`](../windows-rendering-design.md).
- **Window-state persistence.** Window size, position, and maximized state are
  saved and restored across restarts via
  `wezterm-gui/src/window_state_persistence.rs` (state lives in
  `window-state.json`).
- **UX test harness.** A Python harness under `tests/ux/` drives the real
  `weezterm-gui` binary through Win32 APIs to assert resize, maximize, startup,
  and state-persistence behavior; it must be run after changes to window
  management, resize, or DPI handling.

All upstream-file touches follow
[ADR 0001](0001-fork-strategy-and-clean-merge-discipline.md).

## Consequences

- **Positive:** correct, artifact-free rendering across local GPU, virtual GPU,
  and RDP sessions; reliable window-state restore; regressions are caught by an
  automated UX harness.
- **Negative / trade-offs:** three render paths and the DComp work raise the
  Windows maintenance burden and add a wgpu version floor (DirectComposition
  features require a newer wgpu than the current pin); the UX harness is
  Windows-only and cannot run in Linux CI.
- **Follow-ups:** open items and known issues are tracked in
  `tests/ux/FINDINGS.md`; the wgpu upgrade prerequisite is tracked separately.

## Alternatives considered

- *Single render path matching upstream* — rejected: leaves RDP readback
  overhead and artifacts unaddressed.
