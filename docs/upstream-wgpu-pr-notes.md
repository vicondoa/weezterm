# Upstream wgpu PR notes — `Dx12SwapchainScaling::None`

> Tracking note for a future contribution to
> [gfx-rs/wgpu](https://github.com/gfx-rs/wgpu).
>
> Status: **not yet filed**. This document captures the proposal so a
> future contributor (from this fork or elsewhere) can pick it up
> without re-deriving the rationale. Do **not** open a wgpu PR from
> WeezTerm CI without explicit maintainer approval.

## Context

WeezTerm Mode A (DComp) — see `docs/windows-rendering-design.md` §4 —
creates its DX12 swap chain through wgpu-hal with
`presentation_system: Dx12SwapchainKind::DxgiFromVisual`. The DXGI
swap chain that wgpu-hal creates underneath is configured with the
default `DXGI_SCALING_STRETCH` flag (verify by reading
`wgpu-hal/src/dx12/mod.rs` around the
`CreateSwapChainForComposition` / `CreateSwapChainForHwnd` call near
`fn configure_swap_chain`).

`DXGI_SCALING_STRETCH` causes DXGI/DComp to **scale** the previous
frame's content to the new dimensions during a window resize, for as
long as the application has not yet presented a new frame at the new
size. For terminal emulators (and other applications that resize their
content per-cell rather than scaling), this produces a visible
single-frame stretch artifact each time `WM_SIZE` arrives — the
previous frame is squashed/stretched into the new client rect for
~16 ms before the renderer catches up.

`DXGI_SCALING_NONE` instead clips the existing back-buffer content to
the top-left of the new client rect and fills the remainder with the
swap chain's `BackgroundColor`. The resulting transient is "the old
content sitting in its old position with a coloured border around the
new client area" — much less visually jarring than a stretch.

## Proposed API

```rust
// In wgpu-types (wgpu_types::Dx12BackendOptions):

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dx12SwapchainScaling {
    /// `DXGI_SCALING_STRETCH` — the existing default. Old frame is
    /// scaled into the new client rect during resize.
    #[default]
    Stretch,
    /// `DXGI_SCALING_NONE` — old frame is clipped to the top-left of
    /// the new client rect; the remainder is filled with the swap
    /// chain background colour. Recommended for terminal emulators
    /// and other per-cell-resized applications.
    None,
    /// `DXGI_SCALING_ASPECT_RATIO_STRETCH` — preserves aspect ratio
    /// while stretching. Less commonly useful.
    AspectRatioStretch,
}

pub struct Dx12BackendOptions {
    // ... existing fields ...
    pub swap_chain_scaling: Dx12SwapchainScaling,
}
```

## Files to touch (in `wgpu`)

* `wgpu-types/src/lib.rs` — add the enum and the
  `Dx12BackendOptions` field with `#[serde(default)]`.
* `wgpu-hal/src/dx12/instance.rs` — extend the swap chain creation
  call to pass through the chosen `DXGI_SCALING_*` flag.
* `wgpu-hal/src/dx12/mod.rs` — same plumbing for
  `configure_swap_chain` (the `ResizeBuffers` path may need a check
  too — `IDXGISwapChain::ResizeBuffers` does not change the scaling
  flag, so this is mostly a creation-time concern).
* `wgpu/src/lib.rs` — re-export `Dx12SwapchainScaling`.
* CHANGELOG entry.

## Test plan (in wgpu)

There isn't an obvious unit test for visual artifact behaviour, so the
PR description should include:

* Manual repro: create a DXGI swap chain with each scaling mode,
  enter a tight `WM_SIZE` loop (drag the window), screenshot the
  transient.
* Verification that the new field is wired through `wgpu-types` and
  reaches the underlying `DXGI_SWAP_CHAIN_DESC1.Scaling` field.

## WeezTerm cross-reference

Once this lands in a wgpu release WeezTerm depends on:

1. Bump `wgpu` in `Cargo.toml`.
2. In `wezterm-gui/src/termwindow/webgpu.rs`, set
   `swap_chain_scaling: wgpu::Dx12SwapchainScaling::None` inside the
   Mode A `Dx12BackendOptions` block (next to the existing
   `presentation_system` and `latency_waitable_object` fields).
3. Remove the manual HAL-level workaround comment in the same file
   that references this document.
4. Update `docs/windows-rendering-design.md` §4 + §6 Phase 3 to drop
   the "deferred upstream PR" note.

## Why we are not opening this PR right now

* Phase 3 of the WeezTerm rendering rework prioritises shipping the
  HAL-level frame-latency wait, which provides the bulk of the
  perceived improvement on its own.
* Manually rebuilding the swap chain through the wgpu HAL with
  `DXGI_SCALING_NONE` in WeezTerm itself is high-risk: it requires
  duplicating much of `wgpu-hal::dx12::Surface::configure_swap_chain`,
  including the DComp visual binding and the latency-waitable
  bookkeeping. The bug surface that this would introduce is larger
  than the artifact it would fix.
* Filing the upstream PR is best done by someone with `wgpu`
  contributor familiarity who can shepherd the API design discussion
  without blocking on WeezTerm release timing.
