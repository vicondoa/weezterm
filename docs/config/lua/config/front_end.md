---
tags:
  - gpu
---
# `front_end = "OpenGL"`

Specifies which render front-end to use.  This option used to have
more scope in earlier versions of wezterm, but today it allows three
possible values:

* `OpenGL` - use GPU accelerated rasterization
* `Software` - use CPU-based rasterization.
* `WebGpu` - use GPU accelerated rasterization {{since('20221119-145034-49b9839f', inline=True)}}

{{since('20240127-113634-bbcac864', outline=true)}}
    The default is `"WebGpu"`. In earlier versions it was `"OpenGL"`

{{since('20240128-202157-1e552d76', outline=true)}}
    The default has been reverted to `"OpenGL"`.

You may wish (or need!) to select `Software` if there are issues with your
GPU/OpenGL drivers.

WezTerm will automatically select `Software` if it detects that it is
being started in a Remote Desktop environment on Windows.

<!-- --- weezterm remote features --- -->

## WeezTerm rendering modes

WeezTerm extends the upstream `front_end` setting with two additional
variants and a new default-selection policy on Windows:

* **`Auto`** (default on Windows) — selects the best rendering mode for
  the current environment:
    * **Mode C `SoftwareRdp`** in RDP / virtual-GPU sessions: WARP D3D11
      + `Present1` with dirty rectangles. Encoder-friendly, low wire
      bytes, no GPU readback per frame.
    * **Mode A `WgpuDComp`** on Windows 10 19041+ / Windows 11 with a
      real GPU: wgpu DX12 + DirectComposition + premultiplied alpha +
      waitable swap chain. Smooth resize, ≤ 1-frame latency, true
      Mica/Acrylic translucency.
    * **Mode B `WgpuClassic`** on older Windows builds, on machines
      where DComp swap-chain creation fails, or when `Auto` falls back
      from Mode A: wgpu DX12 without DComp.
* **`WebGpuHwnd`** — explicit force of Mode B. Use as a driver-issue
  workaround when `Auto` selects Mode A and renders incorrectly.

The selected mode is logged at startup. Override at runtime via the
`WEEZTERM_RENDER_MODE` environment variable, which accepts one of
`auto`, `wgpu_dcomp`, `wgpu_classic`, `software_rdp`. Useful for A/B
testing and bug reports.

## Deprecated: `OpenGL`

The `OpenGL` (glium) front-end is **deprecated** as of the WeezTerm
rendering overhaul. It does not integrate cleanly with DWM (no
flip-model swap chain, classic stretch on resize, problematic behaviour
on RDP disconnect) and will be removed once Mode C (`SoftwareRdp`) has
shipped for one full release. Migrate to `Auto` (the new default on
Windows). Existing configs with `front_end = "OpenGL"` continue to work
but emit a deprecation warning when compiled.

See `docs/windows-rendering-design.md` for the full design.

<!-- --- end weezterm remote features --- -->

## WebGpu

{{since('20221119-145034-49b9839f')}}

The WebGpu front end allows wezterm to use GPU acceleration provided by
a number of platform-specific backends:

* Metal (on macOS)
* Vulkan
* DirectX 12 (on Windows)

See also:

* [webgpu_preferred_adapter](webgpu_preferred_adapter.md)
* [webgpu_power_preference](webgpu_power_preference.md)
* [webgpu_force_fallback_adapter](webgpu_force_fallback_adapter.md)
