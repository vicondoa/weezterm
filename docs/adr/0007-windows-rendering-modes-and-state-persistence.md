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

- **Three rendering modes** (`window/src/render_mode.rs`): **WgpuDComp** (wgpu
  DX12 + DirectComposition, for modern Win10/11 with a real GPU), **WgpuClassic**
  (wgpu DX12 without DComp), and **SoftwareRdp** (CPU rasteriser + WARP). The
  startup auto-selection (`RenderMode::auto_select`) picks **WgpuDComp** by
  default, falls back to **WgpuClassic** on Win10 < 19041, and — importantly —
  routes **RDP and virtual-GPU sessions to WgpuClassic, not SoftwareRdp**:
  the SoftwareRdp WARP path currently renders as a black box in real RDP
  sessions, so it is **opt-in only** via `WEEZTERM_RENDER_MODE=software_rdp`
  pending the Phase 4c follow-up. Non-Windows always uses WgpuClassic. The
  selection rationale, detection logic, and phased plan live in
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
  and RDP sessions (RDP/virtual-GPU currently served by WgpuClassic); reliable
  window-state restore; regressions are caught by an automated UX harness.
- **Negative / trade-offs:** three render paths and the DComp work raise the
  Windows maintenance burden and add a wgpu version floor (DirectComposition
  features require a newer wgpu than the current pin); the SoftwareRdp WARP path
  is not yet usable in real RDP sessions (black-box bug) and stays opt-in; the UX
  harness is Windows-only and cannot run in Linux CI.
- **Follow-ups:** fix the SoftwareRdp WARP renderer so Auto can prefer it for RDP
  (Phase 4c); open items and known issues are tracked in `tests/ux/FINDINGS.md`;
  the wgpu upgrade prerequisite is tracked separately.

## Alternatives considered

- *Single render path matching upstream* — rejected: leaves RDP readback
  overhead and artifacts unaddressed.
