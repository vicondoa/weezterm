# WeezTerm Windows Rendering — FINAL FIX Design

> **Status:** Design / proposal. Not yet implemented.
> **Audience:** WeezTerm maintainers / contributors.
> **Scope:** Windows rendering quality, especially in Azure VM + RDP environments.
> **Companion docs:** `tests/ux/FINDINGS.md`, `tests/ux/MANUAL_TESTS.md`, `docs/remote-extensions.md`.

---

## 0. TL;DR

The Windows rendering pipeline is "flaky" because **WeezTerm uses the wrong
rendering strategy for every important environment**:

| Environment                                  | What WeezTerm does today                        | What it *should* do                                                                 |
| -------------------------------------------- | ----------------------------------------------- | ----------------------------------------------------------------------------------- |
| **Azure VM + RDP** (this dev machine)        | wgpu → DX12 on Hyper-V virtual GPU              | CPU-side renderer, present via DXGI flip with dirty rects (RDP-encoder friendly)    |
| **Local Win11 with discrete GPU**            | wgpu → DX12, no DComp, `SCALING_STRETCH`        | wgpu → DX12 + DComp (`DxgiFromVisual`) + `SCALING_NONE` + `SetMaximumFrameLatency(1)` |
| **Local Win11 with Mica/Acrylic translucency** | wgpu, `CompositeAlphaMode::Auto`              | wgpu + DComp + `CompositeAlphaMode::PreMultiplied`                                  |
| **Local with `front_end = "OpenGL"`**        | glium → WGL (DWM-incompatible, stretch on resize) | Deprecate / hide; route to wgpu + DComp                                              |

The plan is **not a rewrite**. wgpu already has every primitive we need
(`wgpu-hal/src/dx12/dcomp.rs`, `Dx12SwapchainKind::DxgiFromVisual`,
`SurfaceTargetUnsafe::CompositionVisual`, `CompositeAlphaMode::PreMultiplied`).
We only need to:

1. **Detect the environment** correctly (RDP, virtualized GPU, locally).
2. **Pick the right render mode per environment.**
3. **Wire wgpu's existing DComp/waitable knobs through** instead of relying on
   defaults that were chosen for general-purpose graphics applications.
4. **Patch wgpu where required** (`DXGI_SCALING_NONE`, `Present1` dirty rects).

This is the FINAL design. No more piecemeal handlers. No migration to a new
window/UI framework. Migration is rejected with reasons (§9).

---

## 1. The actual problems (verified)

These are the symptoms — every one of them is documented in
`tests/ux/FINDINGS.md` or directly observable on the dev machine.

1. **Window content stretches/smears during live resize** — the previous frame
   is scaled to fill the new window rect for ~8–16 ms before the GPU presents
   the new frame. Cause: `DXGI_SCALING_STRETCH` + no DWM composition handoff +
   no waitable-object frame pacing.
2. **Window "balloons" when dragged across monitors with different DPI** —
   *partially fixed* in this fork (`window/src/os/windows/window.rs:3219`
   handles `WM_DPICHANGED`). Still some residual artifacts because the wgpu
   surface reconfigure is async.
3. **Transparent / Mica / Acrylic windows render incorrectly** — the wgpu
   surface uses `CompositeAlphaMode::Auto`, which on DX12 maps to
   `DXGI_ALPHA_MODE_IGNORE` (opaque). The DWM backdrop effect therefore renders
   under an opaque buffer and is invisible in most cases.
4. **Black/white flash during maximize/restore** — *partially fixed* by
   `WM_ERASEBKGND` filling with the bg color brush, and by
   `DWMWA_TRANSITIONS_FORCEDISABLED`. Still flashes because the GPU frame is
   not synchronised with the geometry change.
5. **Sluggish, laggy feel in RDP / Azure VM** — wgpu picks the DX12 backend on
   the *Microsoft Hyper-V Video* / *Microsoft Remote Display Adapter*. Every
   frame is rendered on a virtualized GPU, then read back, then re-encoded by
   the RDP H.264 encoder, then sent over the wire. **GPU acceleration in this
   environment is a net negative**: it adds GPU→CPU readback latency on top
   of the encoding latency you'd pay anyway.
6. **Cursor blink / animations stutter under load** — render thread shares the
   UI thread, paint is throttled by `paint_throttled` flag.

---

## 2. The dev machine — why it deserves first-class support

This is where the work is being done; a fix that doesn't help here is no fix.

```
Manufacturer      : Microsoft Corporation
Model             : Virtual Machine                      (Hyper-V VM)
OS                : Windows 11 Enterprise 26200          (Windows 11 24H2+)
Logical CPUs      : 16
RAM               : 64 GB
GPU #1            : Microsoft Hyper-V Video              (synthetic, no real driver)
GPU #2            : Microsoft Remote Display Adapter     (RDP virtual display)
Display (active)  : 2560×1600 @ 96 DPI via the RDP adapter
Session           : rdp-sxs260209400#0  (RDP session)
RDP client        : DESKTOP-QOQLNO7
```

### What this means for rendering

* `GetSystemMetrics(SM_REMOTESESSION)` returns true → WeezTerm's
  `is_running_in_rdp_session()` (`window/src/os/windows/mod.rs:23`) returns
  true.
* For `front_end = "OpenGL"`, WeezTerm correctly forces software rendering via
  `prefer_swrast()` (`window/src/configuration.rs:1`) — but that uses
  **LLVMpipe**, which is a CPU rasterizer that is *also* slow.
* For `front_end = "WebGpu"`, **there is no RDP check at all**. wgpu picks the
  DX12 backend on the virtualized adapter, and we get the worst of both worlds:
  GPU readback every frame plus RDP encoding.
* DComp / DXGI flip-model swap chains are **not the right design point on
  RDP**. RDP's video pipeline doesn't care about flip-model atomicity; it cares
  about *dirty rectangles being small and aligned to the encoder's macroblock
  grid* (16×16 px for H.264).
* The RDP virtual adapter does *not* honour `SetMaximumFrameLatency` or
  composition-bound waitable objects in the same way a discrete GPU does;
  wgpu's frame pacing is essentially advisory in this environment.

### What's available on this machine for rendering

| Capability                                          | Available? | Notes                                                              |
| --------------------------------------------------- | ---------- | ------------------------------------------------------------------ |
| GDI / `BitBlt` / `StretchDIBits`                    | ✅         | Fastest path on RDP (encoder-friendly)                             |
| Direct2D / DirectWrite / DirectComposition          | ✅         | D2D will use WARP under RDP automatically                          |
| DXGI flip-model swap chain on virtual adapter       | ⚠️         | Works but every present causes a readback for the RDP encoder      |
| `Present1` with dirty rectangles                    | ✅         | The RDP encoder benefits massively from this                       |
| Hyper-V GPU passthrough (DDA / GPU-P)               | ❌         | Not configured on this VM; would require Azure VM SKU change       |
| WARP (Windows Advanced Rasterization Platform)      | ✅         | High-quality CPU D3D11/D3D12 fallback; what WeezTerm should target |
| LLVMpipe (Mesa software GL via libEGL)              | ✅         | Currently used for `OpenGL` + RDP; slower than WARP                |

**The clear conclusion**: in RDP, render with WARP + Direct2D-style CPU
rendering, present with `Present1(...)` + dirty rects. Avoid the virtualized
GPU's flip path entirely.

---

## 3. How others solve this

(Verified from source — see citations.)

### Microsoft Windows Terminal — `AtlasEngine` (the gold standard)

`microsoft/terminal:src/renderer/atlas/AtlasEngine.r.cpp`

* D3D11 (or WARP) + `IDXGISwapChain2` with
  `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`
* `DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL`, `DXGI_SCALING_NONE`, `BufferCount = 3`
* `SetMaximumFrameLatency(1)` → input-to-display latency ≤ 1 frame
* Always uses `Present1(...)` with **dirty rects + scroll rects** — this is
  what makes scrolling and typing feel instant
* `BackendD2D.cpp` is a Direct2D fallback that auto-engages when D3D11 is
  unavailable or low-power (perfect for RDP)
* Custom titlebar via `WM_NCCALCSIZE` returning 0 + `DwmDefWindowProc` for
  hit-testing
* Result: by far the smoothest Windows terminal, locally and over RDP.

### Ghostty (macOS) — the resize-safety pattern worth stealing

`ghostty-org/ghostty:src/renderer/metal/IOSurfaceLayer.zig`

* Triple-buffered `IOSurface`s set as `CALayer.contents`
* `contentsGravity = kCAGravityTopLeft` → compositor *clips* on resize, never
  stretches
* All `CALayer` actions overridden to `NSNull` → no animation cross-fade on
  resize
* **Key idea applicable to us**: in the present callback, if the rendered
  surface dimensions don't match the current layer bounds, **drop the frame**
  rather than presenting a wrong-size buffer.

No Windows port exists.

### Kitty / Alacritty

* Pure OpenGL via WGL or X11/Wayland.
* Both have the "stretch during Windows resize" bug; neither uses DComp.
* Not architecturally relevant for solving our problem.

### Zed / GPUI

* Uses wgpu (via `blade`) on Windows with DX12 backend.
* Has an explicit `DISABLE_DIRECT_COMPOSITION` env var → DComp is the default
  but they ship an escape hatch for broken drivers.
* This validates our wgpu-based plan.

---

## 4. The design

We support **three rendering modes**. Each is selected automatically based on
environment, and overridable via config.

```
┌─────────────────────────────────────────────────────────────────┐
│                   FrontEndSelection (config)                    │
│                                                                 │
│   "Auto" (new, default)  →  pick based on environment           │
│        │                                                        │
│        ├─→ in RDP / virtual GPU →  Mode C: SoftwareRdp          │
│        ├─→ Win10 build < 19041   →  Mode B: WgpuClassic         │
│        └─→ Win10 19041+ / Win11  →  Mode A: WgpuDComp           │
│                                                                 │
│   "WebGpu"     →  Mode A (force, even in RDP)                   │
│   "WebGpuHwnd" →  Mode B (force classic, no DComp)              │
│   "Software"   →  Mode C (force CPU + WARP)                     │
│   "OpenGL"     →  legacy glium path; deprecated                 │
└─────────────────────────────────────────────────────────────────┘
```

### Mode A — `WgpuDComp` (local Windows, modern)

> Target: modern Win10/Win11 with a real GPU (discrete or integrated).
> Goal: zero stretch on resize, premultiplied transparency, ≤1 frame latency.

Implementation:

```rust
// wezterm-gui/src/termwindow/webgpu.rs

let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
    backend_options: wgpu::BackendOptions {
        dx12: wgpu::Dx12BackendOptions {
            // wgpu 28+: create swap chain via CreateSwapChainForComposition
            // and attach it to an IDCompositionVisual we own.
            presentation_system: wgt::Dx12SwapchainKind::DxgiFromVisual,
            // Use the IDXGISwapChain2 frame-latency waitable for ≤ 1-frame latency.
            latency_waitable_object: wgt::Dx12UseFrameLatencyWaitableObject::Wait,
            shader_compiler: wgt::Dx12Compiler::DynamicDxc { .. },
            ..Default::default()
        },
        ..Default::default()
    },
    flags: wgpu::InstanceFlags::default(),
});

let config = wgpu::SurfaceConfiguration {
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    format: wgpu::TextureFormat::Bgra8Unorm,           // not _Srgb; SRGB via RTV
    width,
    height,
    present_mode: wgpu::PresentMode::Fifo,
    desired_maximum_frame_latency: 1,                   // was 2
    alpha_mode: if window_uses_translucency() {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else {
        wgpu::CompositeAlphaMode::Opaque                // independent flip when opaque
    },
    view_formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
};
```

Plus, for the bits wgpu doesn't yet expose, drop down to HAL:

```rust
// SAFETY: holds a strong ref to the swap chain; caller guarantees Dx12.
unsafe {
    surface.as_hal::<wgpu::hal::api::Dx12, _, _>(|raw| {
        let raw = raw.expect("dx12 surface");
        let sc2: IDXGISwapChain2 = raw.swap_chain().cast()?;

        // Tell DWM to clip rather than stretch on resize.
        // (Ideally this lands in wgpu via a contributed flag.)
        // Until then, recreate the swap chain with DXGI_SCALING_NONE
        // by reaching into the HAL.

        sc2.SetMaximumFrameLatency(1)?;
        let waitable = sc2.GetFrameLatencyWaitableObject();
        // Stash `waitable`; the render thread waits on it before recording.
    });
}
```

WindowProc changes (already partially in fork; verify they remain correct
under DComp):

* `WM_DPICHANGED` → `SetWindowPos(hwnd, suggested_rect)` (already ✅)
* `WM_ERASEBKGND` → fill with bg brush (already ✅)
* `WM_ENTERSIZEMOVE` / `WM_EXITSIZEMOVE` → bypass `paint_throttled`, schedule
  immediate sync repaint on `WM_EXITSIZEMOVE` (already ✅)
* `DWMWA_TRANSITIONS_FORCEDISABLED` → keep (already ✅)
* `DWMWA_SYSTEMBACKDROP_TYPE` (Mica/Acrylic) → keep, but only when
  `alpha_mode = PreMultiplied` is in effect (NEW: gate it)

### Mode B — `WgpuClassic` (legacy fallback)

> Target: drivers / OS versions where DComp's `DxgiFromVisual` path is buggy.
> This is the existing wgpu path with the DPI/erase fixes.

Same as Mode A but `Dx12SwapchainKind::DxgiFromHwnd`, `Opaque` alpha, no
DComp. Used when:

* Windows 10 < 19041 (build threshold for reliable
  `DCompositionCreateDevice2`), or
* Driver blacklist (TBD; mirror Zed's `DISABLE_DIRECT_COMPOSITION`).

Selectable via `front_end = "WebGpuHwnd"`.

### Mode C — `SoftwareRdp` (Azure VM + RDP, the dev environment)

> Target: any session where `is_running_in_rdp_session()` is true, OR the
> only available GPU is `Microsoft Basic Display`/`Microsoft Hyper-V Video`/
> `Microsoft Remote Display Adapter`.
> Goal: minimum CPU work per frame, encoder-friendly dirty rectangles, no GPU
> readback.

Two sub-options; we ship (C1) first and consider (C2) later.

**C1 — Pure CPU + DXGI present (recommended first cut).**

* Render the terminal to an in-memory `Bgra8` buffer using the existing tiny
  CPU rasteriser already used for some overlays (or a new `softbuffer`-style
  path). Glyphs come from the existing `freetype`/`harfbuzz` atlas, blitted
  into the CPU buffer.
* Create a DXGI swap chain via WARP (`D3D11CreateDevice` with
  `D3D_DRIVER_TYPE_WARP`) — `BufferCount = 2`, `FLIP_SEQUENTIAL`,
  `SCALING_NONE`, alpha `IGNORE`.
* Each frame: `Map` the back buffer, `memcpy` the dirty rectangles from our
  CPU buffer, `Unmap`, `Present1` with the dirty-rect list.
* For the common case where only N cells changed, dirty rects are tiny → RDP
  encoder transmits a few hundred bytes per frame instead of a full 2560×1600
  re-encode.

**C2 — Direct2D + WARP (alternative, slightly heavier).**

* `D2D1CreateDevice` on a WARP `D3D11Device`.
* Use `ID2D1DeviceContext` + custom glyph rendering (DirectWrite glyph runs
  cached in a bitmap atlas).
* Same `Present1` dirty-rect logic.
* Slightly more CPU than (C1) but uses Microsoft's text rasteriser, which is
  the *exact* code that makes Windows Terminal feel native over RDP.

Both C1 and C2 produce identical wire output to the RDP encoder for typical
terminal workloads, because the dirty rects are what dominate.

### Mode-switching policy

```rust
// New: window/src/render_mode.rs
pub enum RenderMode {
    WgpuDComp,
    WgpuClassic,
    SoftwareRdp,
}

pub fn auto_select() -> RenderMode {
    if cfg!(windows) {
        if is_running_in_rdp_session()
            || only_virtual_gpus_available() {
            return RenderMode::SoftwareRdp;
        }
        if windows_build_number() < 19041 {
            return RenderMode::WgpuClassic;
        }
        return RenderMode::WgpuDComp;
    }
    // macOS, Linux: existing wgpu path is fine
    RenderMode::WgpuClassic
}
```

`only_virtual_gpus_available()` enumerates `IDXGIAdapter1` and returns true
if every adapter matches `Microsoft Basic Display`, `Microsoft Hyper-V Video`,
or `Microsoft Remote Display Adapter` (description string match — these names
are stable since Win10).

### Resize / DPI flow under all three modes

The window-proc behaviour is unchanged across modes (these handlers are mode-
agnostic and already in the fork):

```
WM_ENTERSIZEMOVE → in_size_move = true; paint_throttled = false
WM_SIZE          → recompute client rect; queue surface.configure(GetClientRect())
WM_PAINT         → BeginPaint/EndPaint; dispatch NeedRepaint
                   (during in_size_move, do this synchronously, not throttled)
WM_EXITSIZEMOVE  → in_size_move = false; one final synchronous paint
WM_DPICHANGED    → SetWindowPos(suggested rect); rescale font; queue paint
WM_ERASEBKGND    → FillRect with bg_brush; return 1
```

The renderer-side change: each mode is responsible for **dropping frames whose
buffer dimensions don't match the current `GetClientRect`** (the Ghostty
pattern). This single change kills the "smear during fast drag" artifact in
all three modes.

---

## 5. Why this works specifically for the Azure VM + RDP machine

Going through the symptoms on §1 against Mode C:

1. **Stretch on resize** — gone. CPU buffer is always the exact target size;
   when the size changes, we resize the buffer and present it on the next
   frame. Nothing for DWM to stretch.
2. **DPI cross-monitor balloon** — `WM_DPICHANGED` already fixed. CPU
   renderer trivially rescales.
3. **Translucency** — RDP doesn't render Mica/Acrylic anyway (DWM disables
   them in remote sessions). Mode C uses `ALPHA_MODE_IGNORE` and the user gets
   a solid colour, which is *correct* behaviour for RDP.
4. **Maximize/restore flash** — `bg_brush` plus dirty-rect `Present1` means
   the only thing transmitted is the new content; no full-frame flash.
5. **Sluggish RDP feel** — solved. We stop sending full-frame GPU output for
   the encoder to recompress. Per-frame wire bytes drop by 100–1000× for
   typical terminal workloads.
6. **Cursor blink stutter** — separate render thread (§7) ensures the cursor
   timer always fires.

Net effect on the dev machine: **fewer wire bytes per frame, lower
CPU on the encoder side, lower latency, no stretching, no flash.**

---

## 6. Implementation plan

Phases are sized so each one ships independently and is individually testable.

### Phase 0 — Diagnostics & guardrails (no behaviour change)

* Log the selected `RenderMode`, GPU adapter list, RDP-session state, and
  build number at startup.
* Extend `tests/ux/conftest.py` to record the env (RDP / local) for each test
  run so we don't accidentally regress one mode while fixing the other.
* Add a `WEEZTERM_RENDER_MODE` env var override that maps to the same enum
  (cf. Zed's `DISABLE_DIRECT_COMPOSITION`).

### Phase 1 — Mode-selection plumbing

* New `window/src/render_mode.rs` with `RenderMode` enum + `auto_select()`.
* Extend `FrontEndSelection` with `Auto`, `WebGpuHwnd` variants.
  Keep `OpenGL` / `WebGpu` / `Software` for compatibility.
* Make `Auto` the default *only on Windows* (other platforms: keep current
  default).
* No renderer changes yet — `Auto` resolves to today's behaviour for now.

### Phase 2 — Wgpu upgrade (25 → 28+) and DComp wiring (Mode A)

* Bump `wgpu = "28"` (or latest at implementation time) in workspace
  `Cargo.toml`. Track all breaking-change call sites (queue submit signature,
  `RenderPassDescriptor` lifetimes, etc.); these are mechanical.
* In `webgpu.rs`, set
  `Dx12BackendOptions::presentation_system = DxgiFromVisual` and
  `latency_waitable_object = Wait` on Windows.
* Set `desired_maximum_frame_latency = 1`.
* Set `alpha_mode = PreMultiplied` only when the window is configured for
  translucency (Mica/Acrylic backdrop, `window_background_opacity < 1.0`, or
  `WindowDecorations::NONE` with translucency).
* Drop the existing Win10-pre-19041 transparency hack
  (`enable_blur_behind` zero-region trick) when `DxgiFromVisual` is in
  effect; keep it only as Mode B fallback.
* Validate locally on a non-RDP Win11 box. UX tests must pass.

### Phase 3 — Wgpu HAL hooks for SCALING_NONE + waitable

* `surface.as_hal::<Dx12>(...)` to fetch `IDXGISwapChain2`, call
  `SetMaximumFrameLatency(1)` and store the waitable handle in
  `WebGpuState`.
* In the render loop, `WaitForSingleObjectEx(waitable, 100, FALSE)` *before*
  recording the next frame.
* For `DXGI_SCALING_NONE`: file a small wgpu PR exposing
  `Dx12SwapchainScaling::None` (default `Stretch`). Until merged,
  optionally rebuild the swap chain manually via HAL. Mark as P2 — the
  waitable + DComp already remove most of the visible artifact.

### Phase 4 — RDP-friendly software path (Mode C)

* New `wezterm-gui/src/termwindow/software_rdp.rs` (mirrors `webgpu.rs`):
  * `SoftwareRdpState` owns: WARP `ID3D11Device` + `IDXGISwapChain2`,
    a `Vec<u8>` BGRA scratch buffer, dirty-rect tracker.
  * `present(dirty_rects: &[Rect])` does `Map`/`memcpy`/`Unmap`/`Present1`.
* Reuse the existing CPU-side glyph atlas (already maintained for the
  fallback path).
* Hook into `frontend.rs` selection so `RenderMode::SoftwareRdp` constructs
  a `SoftwareRdpState` instead of `WebGpuState`.
* Verify dirty-rect tracker output against a simulated RDP capture
  (PowerShell `mstsc /v:127.0.0.1` against the same VM works for testing).

### Phase 5 — Wrong-size-frame discard (Ghostty pattern)

* Before each present (all modes), compare the buffer size against the
  current `GetClientRect` and **drop the frame** if they differ.
* Schedule a repaint on size mismatch so the new size is rendered next.
* This is ~10 lines and removes the last class of "smear" artifact.

### Phase 6 — Renderer thread (optional, future)

* Move `do_paint_*` off the UI thread onto a dedicated render thread with
  its own `smol` executor.
* Ghostty does this with libxev; `smol` is sufficient for our needs.
* Decouples paint from the WindowProc message loop; eliminates the cursor-
  blink stutter under load.

### Phase 7 — Cleanup

* Mark `front_end = "OpenGL"` as deprecated in docs and config validation.
* Remove the LLVMpipe path once Mode C is shipping for ≥ 1 release.
* Update `tests/ux/FINDINGS.md` to mark issues 5 (content stretching) and
  the RDP issues as resolved.

---

## 7. Risks and mitigations

| Risk                                                       | Likelihood | Mitigation                                                                                          |
| ---------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------- |
| wgpu 25 → 28 breaking changes touch many files             | High       | Mechanical migration; covered by `cargo check`. Don't ship Phase 2 until everything compiles cleanly. |
| `Dx12SwapchainKind::DxgiFromVisual` is undertested         | Medium     | Mode B (`WgpuClassic`) is the explicit escape hatch. `WEEZTERM_RENDER_MODE=WebGpuHwnd` env var.     |
| WARP availability on stripped-down Server SKUs             | Low        | All Azure Windows SKUs ship WARP. Detect and fall back to LLVMpipe if missing.                      |
| Dirty-rect tracking has off-by-one bugs → corrupt display  | Medium     | Add `WEEZTERM_FORCE_FULL_PRESENT=1` debug knob. Snapshot tests in `tests/ux/`.                      |
| Mica/Acrylic regress on Mode A because alpha_mode changed  | Medium     | Phase 2 explicitly only sets `PreMultiplied` when translucency is configured; keep Opaque otherwise. |
| RDP detection misfires (e.g. Parsec, Sunshine, Steam Link) | Low        | These set `SM_REMOTESESSION = 0`. Add adapter-name heuristic. User can override with config.        |

---

## 8. Testing plan

### Automated

`tests/ux/` already has the harness. New suites:

* `test_render_mode.py` — assert auto-detection picks Mode C in RDP and
  Mode A locally; verify env-var override works.
* `test_resize.py` — extend with assertion that no pixel in the title-bar
  region changes during a resize (no stretch / smear).
* `test_dirty_rect.py` (Mode C only) — render a single character, present,
  capture wire bytes, assert ≤ N bytes changed (proxy for RDP encoder load).
* `test_transparency.py` — Mode A: assert configured Mica backdrop visible
  through window. Mode C: assert solid bg colour (translucency suppressed).
* `test_dpi.py` — drag between two virtual monitors with different DPIs;
  assert no balloon, content sized correctly. Already partially covered.

### Manual

Add to `tests/ux/MANUAL_TESTS.md`:

* **M6** — Connect to the dev VM with RDP at three different bandwidths
  (LAN, 50 Mbps, 5 Mbps); verify smooth typing and scrolling under each.
* **M7** — Compare side-by-side with Windows Terminal in the same RDP
  session. Subjective quality should match.

### CI

`ci/build-cross.sh` already covers the build matrix. Add Mode-A/B/C smoke
tests gated on Windows runners.

---

## 9. Alternatives considered (and rejected)

| Alternative                                              | Why rejected                                                                                              |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Full migration to `windows-rs` + D2D + DWrite            | Windows-only renderer; massive cross-platform divergence; doubles maintenance burden                      |
| Port Microsoft `AtlasEngine` to Rust                     | Tightly coupled to Windows Terminal's text-buffer model; reimplementing wgpu-hal/dx12 functionality       |
| Migrate to `winit` for windowing                         | Loses our custom WndProc (we need it for `WM_NCCALCSIZE`, `WM_DPICHANGED`, `WM_ERASEBKGND` overrides)     |
| Migrate to `egui` / `iced` / `tauri`                     | These are app frameworks, not terminal renderers; they're built *on top of* wgpu — same problems          |
| Drop wgpu, use `softbuffer` only                         | Loses GPU acceleration on the *non-RDP* path; macOS/Linux would suffer                                    |
| Use ANGLE (Google's GLES → DX) instead of native wgpu    | Adds a dependency; same DComp/scaling issues; not a Rust-friendly stack                                   |
| Use Vello (`linebender/vello`)                           | Alpha-quality on Windows; no terminal-specific test coverage; built on wgpu — gains nothing               |
| Configure the Azure VM with GPU passthrough (DDA / GPU-P) | Requires SKU change ($$); doesn't help any user who isn't on a GPU-passed VM                              |

---

## 10. References

### WeezTerm internal

* `window/src/os/windows/window.rs` — WindowProc (all `WM_*` handlers)
* `window/src/os/windows/mod.rs:23` — `is_running_in_rdp_session()`
* `window/src/configuration.rs:1` — `prefer_swrast()`
* `wezterm-gui/src/termwindow/webgpu.rs` — wgpu surface lifecycle
* `wezterm-gui/src/termwindow/render/draw.rs` — per-frame draw
* `wezterm-gui/src/termwindow/resize.rs` — resize / DPI flow
* `tests/ux/FINDINGS.md` — known issues catalogue

### External

* Microsoft Windows Terminal — `microsoft/terminal:src/renderer/atlas/`
  * `AtlasEngine.r.cpp:_createSwapChain` — canonical FLIP_SEQUENTIAL +
    waitable + premultiplied recipe
  * `AtlasEngine.r.cpp:_present` — `Present1` with dirty rects
  * `BackendD2D.cpp` — D2D fallback (the closest reference for our Mode C)
* wgpu — `gfx-rs/wgpu`
  * `wgpu-hal/src/dx12/dcomp.rs` — full IDCompositionDevice/Visual/Target impl
  * `wgpu-hal/src/dx12/instance.rs:142-160` — `Dx12SwapchainKind` dispatch
  * `wgpu/src/api/surface.rs` — `SurfaceTargetUnsafe::CompositionVisual`
* Ghostty — `ghostty-org/ghostty`
  * `src/renderer/metal/IOSurfaceLayer.zig:80-108` — wrong-size frame discard
  * `src/renderer/Thread.zig:19-64` — dedicated render thread w/ libxev
* Zed — `zed-industries/zed`
  * `crates/gpui_windows/src/directx_renderer.rs` — D3D11 + DComp + escape hatch
* Microsoft docs
  * "DirectComposition" — <https://learn.microsoft.com/en-us/windows/win32/directcomp/directcomposition-portal>
  * "Detecting the Terminal Services Environment" —
    <https://learn.microsoft.com/en-us/windows/win32/termserv/detecting-the-terminal-services-environment>
  * "WARP" — <https://learn.microsoft.com/en-us/windows/win32/direct3darticles/directx-warp>
  * "DXGI flip model" —
    <https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model>

---

## Appendix A — Decision matrix at a glance

```
                  RDP / virtual GPU      Local Win11+GPU      Local Win10 19041+
                  -------------------    -----------------    --------------------
Mode               C SoftwareRdp          A WgpuDComp           A WgpuDComp
Backend            WARP via D3D11         wgpu DX12 + DComp     wgpu DX12 + DComp
Swap chain         FLIP_SEQUENTIAL        FLIP_SEQUENTIAL       FLIP_SEQUENTIAL
Scaling            NONE                   NONE                  NONE
Buffers            2                      3                     3
Alpha mode         IGNORE                 PREMULTIPLIED*        PREMULTIPLIED*
Frame latency      n/a (no waitable)      Waitable, max=1       Waitable, max=1
Present            Present1 + dirtyrects  Present1 (full frame) Present1 (full frame)
Dirty rects        YES (essential)        future                future
GPU readback       NONE                   NONE                  NONE
DComp              NO                     YES                   YES
Mica/Acrylic       suppressed             when configured       when configured

*Opaque when window is fully opaque; PreMultiplied unlocks independent flips
 only matters when translucency is configured.
```

## Appendix B — One-time migration checklist

* [ ] Phase 0: logging + env-var override land on `main`
* [ ] Phase 1: `RenderMode` enum + `Auto` selection plumbed (no behaviour change)
* [ ] Phase 2: wgpu 28 + DComp visible on local box
* [ ] Phase 3: HAL waitable + frame-latency=1 land
* [ ] Phase 4: SoftwareRdp shipped, validated on Azure VM via RDP
* [ ] Phase 5: wrong-size-frame discard
* [ ] Phase 6: render thread (optional)
* [ ] Phase 7: deprecate OpenGL front-end; remove LLVMpipe path
