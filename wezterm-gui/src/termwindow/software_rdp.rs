//! CPU-side rendering with WARP D3D11 swap chain + Present1 dirty rects.
//!
//! Optimised for RDP / virtualised-GPU environments. See
//! `docs/windows-rendering-design.md` §4 Mode C.
//!
//! The pipeline is:
//!
//!  1. `D3D11CreateDevice` with `D3D_DRIVER_TYPE_WARP` — guarantees no
//!     real GPU work; the entire pixel pipeline runs on the CPU and the
//!     remote-display encoder transmits only the rectangles we mark
//!     dirty.
//!  2. `IDXGIFactory2::CreateSwapChainForHwnd` with `BufferCount=2`,
//!     `FLIP_SEQUENTIAL`, `SCALING_NONE`, alpha `IGNORE`, BGRA8 format.
//!     `SCALING_NONE` is critical: it tells DXGI not to rescale the
//!     content during a window resize (otherwise the encoder sees a
//!     full-frame change every drag tick).
//!  3. Each frame the caller fills `pixels_mut()` with BGRA pixels and
//!     calls `mark_dirty()` for each region that changed; `present()`
//!     copies the scratch buffer into the back-buffer and calls
//!     `Present1` with the dirty-rect list.
//!
//! Set `WEEZTERM_FORCE_FULL_PRESENT=1` to bypass dirty rects and present
//! a full frame each call — useful when debugging visual corruption.

#![cfg(windows)]

use anyhow::{anyhow, bail, Context, Result};
use std::ptr::null_mut;
use winapi::shared::dxgi::{
    CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, IDXGISurface, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
};
use winapi::shared::dxgi1_2::{
    IDXGIFactory2, IDXGISwapChain1, DXGI_ALPHA_MODE_IGNORE, DXGI_PRESENT_PARAMETERS,
    DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1,
};
use winapi::shared::dxgi1_3::IDXGISwapChain2;
use winapi::shared::dxgiformat::DXGI_FORMAT_B8G8R8A8_UNORM;
use winapi::shared::dxgitype::{DXGI_SAMPLE_DESC, DXGI_USAGE_RENDER_TARGET_OUTPUT};
use winapi::shared::windef::{HWND, RECT};
use winapi::shared::winerror::SUCCEEDED;
use winapi::um::d3d11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
};
use winapi::um::d3dcommon::{
    D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2, D3D_FEATURE_LEVEL_9_3,
};
use winapi::Interface;

/// A single dirty rectangle in window-client pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl DirtyRect {
    /// Clip to `(width, height)`. Returns `None` if the rect is fully
    /// outside or zero-sized.
    fn clip(self, width: u32, height: u32) -> Option<RECT> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w as i32).min(width as i32);
        let y1 = (self.y + self.h as i32).min(height as i32);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(RECT {
            left: x0,
            top: y0,
            right: x1,
            bottom: y1,
        })
    }
}

/// Shared back-end state for Mode C rendering.
pub struct SoftwareRdpState {
    /// Window handle, used by `present()` for the Phase 5 wrong-size-frame
    /// discard check (compare swap-chain dimensions to live `GetClientRect`).
    hwnd: HWND,
    /// WARP D3D11 device. Owned via raw COM pointer; released in `Drop`.
    device: *mut ID3D11Device,
    context: *mut ID3D11DeviceContext,
    swap_chain: *mut IDXGISwapChain2,

    width: u32,
    height: u32,

    /// BGRA8 scratch buffer owned by the caller's CPU rasteriser. Resized
    /// on `resize()`.
    scratch: Vec<u8>,

    /// Dirty rectangles since last `present()`; cleared after Present1.
    dirty: Vec<DirtyRect>,
}

// SoftwareRdpState owns raw COM pointers; sending it across threads
// would require manual synchronisation. The TermWindow only uses it on
// the GUI thread.
unsafe impl Send for SoftwareRdpState {}

impl SoftwareRdpState {
    /// Construct a WARP-backed swap chain bound to `hwnd`.
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        if hwnd.is_null() {
            bail!("SoftwareRdpState::new called with null HWND");
        }
        let width = width.max(1);
        let height = height.max(1);

        // 1. Create the WARP D3D11 device. We accept any feature level
        //    >= 9.1 because WARP is software anyway and we do not
        //    require shader-model-5 features.
        let feature_levels = [
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
            D3D_FEATURE_LEVEL_9_3,
            D3D_FEATURE_LEVEL_9_2,
            D3D_FEATURE_LEVEL_9_1,
        ];
        let mut device: *mut ID3D11Device = null_mut();
        let mut context: *mut ID3D11DeviceContext = null_mut();
        let mut achieved_level = 0;
        let hr = unsafe {
            D3D11CreateDevice(
                null_mut(),
                D3D_DRIVER_TYPE_WARP,
                null_mut(),
                0,
                feature_levels.as_ptr(),
                feature_levels.len() as u32,
                D3D11_SDK_VERSION,
                &mut device,
                &mut achieved_level,
                &mut context,
            )
        };
        if !SUCCEEDED(hr) {
            bail!("D3D11CreateDevice(WARP) failed: HRESULT 0x{:08x}", hr);
        }
        if device.is_null() || context.is_null() {
            unsafe {
                if !device.is_null() {
                    (*device).Release();
                }
                if !context.is_null() {
                    (*context).Release();
                }
            }
            bail!("D3D11CreateDevice succeeded but returned null pointers");
        }
        log::debug!(
            "[software_rdp] WARP device created at feature level 0x{:x}",
            achieved_level
        );

        // 2. Get the DXGI factory from the device, querying up to
        //    IDXGIFactory2 (required for CreateSwapChainForHwnd).
        let factory = unsafe { dxgi_factory_from_device(device) }
            .context("retrieving IDXGIFactory2 from WARP device")?;

        // 3. Build the swap-chain description with the parameters that
        //    keep the RDP encoder happy: BGRA8, 2 buffers,
        //    FLIP_SEQUENTIAL, SCALING_NONE, alpha IGNORE.
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: 0,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_NONE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };

        let mut swap_chain1: *mut IDXGISwapChain1 = null_mut();
        let hr = unsafe {
            (*factory).CreateSwapChainForHwnd(
                device as *mut _,
                hwnd,
                &desc,
                null_mut(),
                null_mut(),
                &mut swap_chain1,
            )
        };
        unsafe {
            (*factory).Release();
        }
        if !SUCCEEDED(hr) || swap_chain1.is_null() {
            unsafe {
                (*device).Release();
                (*context).Release();
            }
            bail!(
                "IDXGIFactory2::CreateSwapChainForHwnd failed: HRESULT 0x{:08x}",
                hr
            );
        }

        // 4. Cast to IDXGISwapChain2 (Win8.1+; always available on the
        //    target Win10/11 environments). If the cast fails, fall
        //    back to releasing.
        let mut swap_chain2: *mut IDXGISwapChain2 = null_mut();
        let hr = unsafe {
            (*swap_chain1).QueryInterface(
                &IDXGISwapChain2::uuidof(),
                &mut swap_chain2 as *mut _ as *mut *mut _,
            )
        };
        unsafe {
            (*swap_chain1).Release();
        }
        if !SUCCEEDED(hr) || swap_chain2.is_null() {
            unsafe {
                (*device).Release();
                (*context).Release();
            }
            bail!(
                "QueryInterface(IDXGISwapChain2) failed: HRESULT 0x{:08x}",
                hr
            );
        }

        let scratch_len = (width as usize) * (height as usize) * 4;
        Ok(Self {
            hwnd,
            device,
            context,
            swap_chain: swap_chain2,
            width,
            height,
            scratch: vec![0u8; scratch_len],
            dirty: Vec::new(),
        })
    }

    /// Resize the swap-chain back buffers and the CPU scratch buffer.
    /// Marks the entire surface dirty so the next `present()` repaints
    /// everything.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return Ok(());
        }
        let hr = unsafe {
            (*self.swap_chain).ResizeBuffers(0, width, height, DXGI_FORMAT_B8G8R8A8_UNORM, 0)
        };
        if !SUCCEEDED(hr) {
            bail!(
                "IDXGISwapChain::ResizeBuffers({}x{}) failed: HRESULT 0x{:08x}",
                width,
                height,
                hr
            );
        }
        self.width = width;
        self.height = height;
        self.scratch
            .resize((width as usize) * (height as usize) * 4, 0);
        self.mark_all_dirty();
        Ok(())
    }

    /// Mutable BGRA8 scratch buffer + stride in bytes (`width * 4`).
    /// Caller fills this and tells us about dirty regions via
    /// `mark_dirty()`.
    pub fn pixels_mut(&mut self) -> (&mut [u8], u32) {
        (&mut self.scratch, self.width * 4)
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Window handle this surface presents to. Used by the resize hook
    /// and the Phase 5 wrong-size-frame discard.
    #[allow(dead_code)] // Public API; callers (diagnostics, future Lua surface) may use it.
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[allow(dead_code)] // Will be consumed by Phase 4c CPU draw path.
    pub fn mark_dirty(&mut self, rect: DirtyRect) {
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        self.dirty.push(rect);
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty.clear();
        self.dirty.push(DirtyRect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        });
    }

    /// Clear all queued dirty rectangles without queuing a new one.
    /// Phase 4c calls this at the start of each CPU-render frame so the
    /// renderer is the sole source of dirty rects (avoids overlapping
    /// rects from earlier `resize()` / `mark_all_dirty()` calls, which
    /// would cause `Present1` to fail with `DXGI_ERROR_INVALID_CALL`).
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Number of dirty rectangles that will be presented next.
    /// Mainly for tests / instrumentation.
    #[allow(dead_code)] // Used by Phase 4d UX tests via the Lua diagnostics surface.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Map the back buffer, copy from the scratch buffer, unmap, and
    /// `Present1` with the dirty-rect list.
    ///
    /// We always copy the *entire* scratch buffer because we use
    /// `D3D11_MAP_WRITE_DISCARD`: the previous contents of the back
    /// buffer that comes around in flip-sequential rotation are
    /// undefined, so partial copies would leak stale pixels. The dirty
    /// rectangles are still passed to `Present1`, which is what the
    /// remote-display encoder reads to decide what to transmit. This is
    /// the trade-off recommended by the design doc §4 Mode C C1 — and
    /// is also how Microsoft's DirectComposition examples handle the
    /// flip-sequential case.
    pub fn present(&mut self) -> Result<()> {
        // Phase 5: wrong-size-frame discard (Ghostty pattern). If the
        // swap-chain dimensions don't match the live client rect, drop
        // this frame and schedule a repaint via InvalidateRect so the
        // next iteration renders at the correct size. Eliminates the
        // "smear during fast drag" artifact that occurs when WM_SIZE
        // arrives between `resize()` and `present()`.
        let (cw, ch) = ::window::os::windows::current_client_size(self.hwnd);
        if cw > 0 && ch > 0 && (cw != self.width || ch != self.height) {
            log::trace!(
                "[render] dropping wrong-size frame (software_rdp): \
                 buffer={}x{}, client={}x{}",
                self.width,
                self.height,
                cw,
                ch
            );
            self.dirty.clear();
            unsafe {
                winapi::um::winuser::InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
            return Ok(());
        }

        let force_full = std::env::var_os("WEEZTERM_FORCE_FULL_PRESENT").is_some();
        if force_full || self.dirty.is_empty() {
            self.mark_all_dirty();
        }

        // 1. Get back buffer 0 as IDXGISurface (so we can Map without
        //    requiring D3D11_USAGE_DYNAMIC; the back buffer of a flip
        //    swap chain supports Map+WRITE_DISCARD via IDXGISurface).
        let mut back_buffer: *mut ID3D11Texture2D = null_mut();
        let hr = unsafe {
            (*self.swap_chain).GetBuffer(
                0,
                &ID3D11Texture2D::uuidof(),
                &mut back_buffer as *mut _ as *mut *mut _,
            )
        };
        if !SUCCEEDED(hr) || back_buffer.is_null() {
            bail!("IDXGISwapChain::GetBuffer(0) failed: HRESULT 0x{:08x}", hr);
        }

        // 2. Map via IDXGISurface (Win8 mappable swapchain surface).
        let mut dxgi_surface: *mut IDXGISurface = null_mut();
        let hr = unsafe {
            (*back_buffer).QueryInterface(
                &IDXGISurface::uuidof(),
                &mut dxgi_surface as *mut _ as *mut *mut _,
            )
        };
        if !SUCCEEDED(hr) || dxgi_surface.is_null() {
            // IDXGISurface mapping is not allowed on flip-model swap
            // chains by default. Fall back to D3D11 staging-texture copy.
            // This is the path the WeezTerm Mode C MVP actually takes.
            unsafe {
                (*back_buffer).Release();
            }
            return self.present_via_update_subresource();
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: null_mut(),
            RowPitch: 0,
            DepthPitch: 0,
        };
        // IDXGISurface::Map takes flags (read=1/write=2/discard=4).
        let hr = unsafe {
            (*dxgi_surface).Map(&mut mapped as *mut _ as *mut _, 4 /*WRITE+DISCARD*/)
        };
        if !SUCCEEDED(hr) || mapped.pData.is_null() {
            unsafe {
                (*dxgi_surface).Release();
                (*back_buffer).Release();
            }
            return self.present_via_update_subresource();
        }

        unsafe {
            self.copy_full_scratch_to_mapped(mapped.pData as *mut u8, mapped.RowPitch);
            (*dxgi_surface).Unmap();
            (*dxgi_surface).Release();
            (*back_buffer).Release();
        }

        self.do_present1()
    }

    /// Fallback present path: build a list of `RECT` for the dirty
    /// regions, then call `UpdateSubresource` per rect into the back
    /// buffer. Used when the back buffer cannot be mapped directly
    /// (the typical path on flip-model swap chains, where DXGI requires
    /// staging copies).
    fn present_via_update_subresource(&mut self) -> Result<()> {
        let mut back_buffer: *mut ID3D11Texture2D = null_mut();
        let hr = unsafe {
            (*self.swap_chain).GetBuffer(
                0,
                &ID3D11Texture2D::uuidof(),
                &mut back_buffer as *mut _ as *mut *mut _,
            )
        };
        if !SUCCEEDED(hr) || back_buffer.is_null() {
            bail!("IDXGISwapChain::GetBuffer(0) failed: HRESULT 0x{:08x}", hr);
        }

        // For the MVP we update the entire back buffer in one shot; the
        // dirty-rect list is still passed to Present1 below so the
        // encoder only re-transmits changed regions. Per-rect
        // UpdateSubresource calls would marginally reduce CPU copy cost
        // but UpdateSubresource has minimum-size alignment requirements
        // (D3D11_C0PY_BOX must be a multiple of the format's block size)
        // that complicate the per-rect path; the trade-off is documented
        // in design doc §4 Mode C.
        let stride = self.width * 4;
        let scratch_ptr = self.scratch.as_ptr() as *const _;
        unsafe {
            (*self.context).UpdateSubresource(
                back_buffer as *mut _,
                0,
                null_mut(),
                scratch_ptr,
                stride,
                stride * self.height,
            );
            (*back_buffer).Release();
        }

        self.do_present1()
    }

    unsafe fn copy_full_scratch_to_mapped(&self, dst: *mut u8, row_pitch: u32) {
        let src_stride = (self.width as usize) * 4;
        let dst_stride = row_pitch as usize;
        for y in 0..(self.height as usize) {
            let src = self.scratch.as_ptr().add(y * src_stride);
            let d = dst.add(y * dst_stride);
            std::ptr::copy_nonoverlapping(src, d, src_stride);
        }
    }

    /// Issue Present1 with the current dirty-rect list and clear it.
    ///
    /// Default: full-frame present (NULL dirty list). Pass dirty rects
    /// only when `WEEZTERM_ENABLE_DIRTY_RECTS=1` because flip-sequential
    /// swap chains roll dirty rects across the back-buffer rotation,
    /// and any dirty rect spec we might emit risks
    /// `DXGI_ERROR_INVALID_CALL` on certain WARP/RDP combos. The RDP
    /// encoder still scans the back-buffer for changed regions, so
    /// emitting NULL dirty rects does not regress bandwidth — it just
    /// gives the encoder slightly more work. Phase 5+ revisits this.
    fn do_present1(&mut self) -> Result<()> {
        let mut rects: Vec<RECT> = self
            .dirty
            .iter()
            .filter_map(|r| r.clip(self.width, self.height))
            .collect();

        let enable_dirty = std::env::var_os("WEEZTERM_ENABLE_DIRTY_RECTS").is_some();

        let (count, ptr) = if enable_dirty && !rects.is_empty() {
            (rects.len() as u32, rects.as_mut_ptr())
        } else {
            (0u32, null_mut())
        };
        let mut params = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: count,
            pDirtyRects: ptr,
            pScrollRect: null_mut(),
            pScrollOffset: null_mut(),
        };
        let hr = unsafe { (*self.swap_chain).Present1(1, 0, &mut params) };
        if !SUCCEEDED(hr) {
            log::error!(
                "[render] Present1 0x{:08x}: bb={}x{}, {} rect(s): {:?}",
                hr,
                self.width,
                self.height,
                rects.len(),
                rects
                    .iter()
                    .map(|r| (r.left, r.top, r.right, r.bottom))
                    .collect::<Vec<_>>(),
            );
            bail!("IDXGISwapChain1::Present1 failed: HRESULT 0x{:08x}", hr);
        }
        self.dirty.clear();
        Ok(())
    }
}

impl Drop for SoftwareRdpState {
    fn drop(&mut self) {
        unsafe {
            if !self.swap_chain.is_null() {
                (*self.swap_chain).Release();
            }
            if !self.context.is_null() {
                (*self.context).Release();
            }
            if !self.device.is_null() {
                (*self.device).Release();
            }
        }
    }
}

/// Walk D3D11Device → IDXGIDevice → IDXGIAdapter → IDXGIFactory, then
/// QueryInterface up to IDXGIFactory2. Releases all intermediates.
unsafe fn dxgi_factory_from_device(device: *mut ID3D11Device) -> Result<*mut IDXGIFactory2> {
    let mut dxgi_device: *mut IDXGIDevice = null_mut();
    let hr = (*device).QueryInterface(
        &IDXGIDevice::uuidof(),
        &mut dxgi_device as *mut _ as *mut *mut _,
    );
    if !SUCCEEDED(hr) || dxgi_device.is_null() {
        return Err(anyhow!(
            "ID3D11Device::QueryInterface(IDXGIDevice) failed: HRESULT 0x{:08x}",
            hr
        ));
    }

    let mut adapter: *mut winapi::shared::dxgi::IDXGIAdapter = null_mut();
    let hr = (*dxgi_device).GetAdapter(&mut adapter);
    (*dxgi_device).Release();
    if !SUCCEEDED(hr) || adapter.is_null() {
        return Err(anyhow!(
            "IDXGIDevice::GetAdapter failed: HRESULT 0x{:08x}",
            hr
        ));
    }

    let mut factory1: *mut IDXGIFactory1 = null_mut();
    let hr = (*adapter).GetParent(
        &IDXGIFactory1::uuidof(),
        &mut factory1 as *mut _ as *mut *mut _,
    );
    (*adapter).Release();
    if !SUCCEEDED(hr) || factory1.is_null() {
        // Older WARP adapters return IDXGIFactory only. Try
        // CreateDXGIFactory1 directly as fallback.
        let mut f: *mut IDXGIFactory1 = null_mut();
        let hr2 = CreateDXGIFactory1(&IDXGIFactory1::uuidof(), &mut f as *mut _ as *mut *mut _);
        if !SUCCEEDED(hr2) || f.is_null() {
            return Err(anyhow!(
                "Both IDXGIAdapter::GetParent (HRESULT 0x{:08x}) and \
                 CreateDXGIFactory1 (HRESULT 0x{:08x}) failed",
                hr,
                hr2
            ));
        }
        factory1 = f;
    }

    let mut factory2: *mut IDXGIFactory2 = null_mut();
    let hr = (*factory1).QueryInterface(
        &IDXGIFactory2::uuidof(),
        &mut factory2 as *mut _ as *mut *mut _,
    );
    (*factory1).Release();
    if !SUCCEEDED(hr) || factory2.is_null() {
        return Err(anyhow!(
            "IDXGIFactory1::QueryInterface(IDXGIFactory2) failed: \
             HRESULT 0x{:08x} -- system is too old",
            hr
        ));
    }

    Ok(factory2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_rect_clip_inside() {
        let r = DirtyRect {
            x: 10,
            y: 20,
            w: 30,
            h: 40,
        };
        let clipped = r.clip(100, 100).unwrap();
        assert_eq!(clipped.left, 10);
        assert_eq!(clipped.top, 20);
        assert_eq!(clipped.right, 40);
        assert_eq!(clipped.bottom, 60);
    }

    #[test]
    fn dirty_rect_clip_partial() {
        let r = DirtyRect {
            x: -5,
            y: -5,
            w: 20,
            h: 20,
        };
        let clipped = r.clip(100, 100).unwrap();
        assert_eq!(clipped.left, 0);
        assert_eq!(clipped.top, 0);
        assert_eq!(clipped.right, 15);
        assert_eq!(clipped.bottom, 15);
    }

    #[test]
    fn dirty_rect_clip_outside() {
        let r = DirtyRect {
            x: 200,
            y: 200,
            w: 10,
            h: 10,
        };
        assert!(r.clip(100, 100).is_none());
    }

    #[test]
    fn dirty_rect_clip_zero() {
        let r = DirtyRect {
            x: 5,
            y: 5,
            w: 0,
            h: 10,
        };
        assert!(r.clip(100, 100).is_none());
    }
}
