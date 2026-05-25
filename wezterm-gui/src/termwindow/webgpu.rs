use crate::quad::Vertex;
use anyhow::anyhow;
use config::{ConfigHandle, GpuInfo, WebGpuPowerPreference};
use std::cell::RefCell;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use window::bitmaps::Texture2d;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use window::{BitmapImage, Dimensions, Rect, Window};

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniform {
    pub foreground_text_hsb: [f32; 3],
    pub milliseconds: u32,
    pub projection: [[f32; 4]; 4],
    // sampler2D atlas_nearest_sampler;
    // sampler2D atlas_linear_sampler;
}

pub struct WebGpuState {
    pub adapter_info: wgpu::AdapterInfo,
    pub downlevel_caps: wgpu::DownlevelCapabilities,
    pub surface: RefCell<Option<wgpu::Surface<'static>>>,
    pub device: wgpu::Device,
    pub queue: Arc<wgpu::Queue>,
    pub config: RefCell<wgpu::SurfaceConfiguration>,
    pub dimensions: RefCell<Dimensions>,
    pub render_pipeline: wgpu::RenderPipeline,
    shader_uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_nearest_sampler: wgpu::Sampler,
    pub texture_linear_sampler: wgpu::Sampler,
    pub handle: RawHandlePair,
    // --- weezterm remote features ---
    /// wgpu instance kept around so we can drop+recreate the surface
    /// on a grow. wgpu's in-place `ResizeBuffers` path leaves a
    /// stretched stale front buffer visible under WARP/RDP — the only
    /// reliable workaround is destroying the swap chain entirely.
    pub instance: wgpu::Instance,
    // --- end weezterm remote features ---
    // --- weezterm remote features ---
    /// Render mode this surface was configured for. Mode A
    /// (`WgpuDComp`) uses DComp + waitable + frame-latency=1; Mode B
    /// (`WgpuClassic`) preserves the historical HWND swapchain
    /// behaviour. See `docs/windows-rendering-design.md` §4.
    pub mode: ::window::render_mode::RenderMode,
    /// Phase 3: cached `HANDLE` returned by
    /// `IDXGISwapChain2::GetFrameLatencyWaitableObject`, fetched via
    /// the wgpu HAL after surface configuration. Only `Some` when
    /// `mode == RenderMode::WgpuDComp` AND the HAL accessor for the
    /// underlying swap chain succeeded.
    ///
    /// The render loop waits on it before recording each frame.
    /// `Drop` calls `CloseHandle` to release our reference; the swap
    /// chain owns its own internal reference, which wgpu-hal closes
    /// when the swap chain is destroyed. See
    /// `docs/windows-rendering-design.md` §4 +
    /// `tests/ux/test_frame_latency.py`.
    #[cfg(windows)]
    pub frame_latency_waitable: Option<winapi::shared::ntdef::HANDLE>,
    /// Coalesced/deferred surface configuration. Calling
    /// `surface.configure` invokes DXGI's `ResizeBuffers`, which on a
    /// virtual GPU (Microsoft Basic Render Driver under RDP, Hyper-V
    /// vGPU, etc.) can cost 100-1700ms per call. Live drag generates
    /// a WM_SIZE per pixel which, if eagerly forwarded, makes the
    /// whole UI thread block for hundreds of milliseconds.
    ///
    /// We instead store the desired dims here and apply them lazily
    /// via `apply_pending_resize()` immediately before
    /// `get_current_texture` in the render path. This naturally
    /// coalesces N WM_SIZE events into 1 ResizeBuffers per frame.
    pub pending_resize: RefCell<Option<Dimensions>>,
    /// Time of the most recent `resize()` call. Kept as a diagnostic
    /// — exposed for debug/log call sites and may inform future
    /// debounce policy. The current `apply_pending_resize` does NOT
    /// debounce on this (the slow `surface.configure` is dispatched
    /// to a background thread instead — see `pending_configure`).
    pub last_resize_request: RefCell<Option<std::time::Instant>>,
    /// True while the window is in a `WM_ENTERSIZEMOVE` /
    /// `WM_EXITSIZEMOVE` interactive drag. We only run
    /// `surface.configure` after the drag ends — DWM stretches the
    /// existing swapchain image during the drag, which on a slow
    /// virtual GPU is far less jarring than blocking the UI thread
    /// for ~1 s per intermediate size.
    pub live_resize_active: RefCell<bool>,
    /// In-flight async surface configure. When `Some`, a background
    /// thread is currently running `surface.configure(...)` for the
    /// dimensions in `target_dims`. The `rx` end yields the
    /// configured surface back to the GUI thread; we swap it into
    /// `self.surface` on the next paint that drains the channel.
    ///
    /// On WARP/RDP, `surface.configure(...)` can take 3+ seconds —
    /// running it on the GUI thread blocks every other smol task
    /// (including the codec read pump that keeps the SSH mux alive),
    /// causing the remote `wezterm-mux-server` to drop us with
    /// `os error 10054` after ~1s. Pushing it to a background thread
    /// keeps the GUI fully interactive while DWM stretches the old
    /// swapchain image to fill the new client rect for the duration.
    pub pending_configure: RefCell<Option<PendingConfigure>>,
    /// Newer dimensions that arrived while a `pending_configure` was
    /// already in flight for different dims. We can't cancel the
    /// in-flight configure, so we queue the latest target and start
    /// another bg configure as soon as the current one lands.
    pub queued_configure_dims: RefCell<Option<Dimensions>>,
    // --- end weezterm remote features ---
}

// --- weezterm remote features ---
/// Handle to an in-flight background `surface.configure(...)` call.
/// See `WebGpuState::pending_configure`.
pub struct PendingConfigure {
    /// Dimensions that the bg thread is configuring the new surface
    /// for. Stored so that:
    ///   * a follow-up resize event for the SAME dims is a no-op
    ///   * a follow-up resize event for DIFFERENT dims goes to
    ///     `queued_configure_dims` and starts another configure when
    ///     this one completes
    pub target_dims: Dimensions,
    /// The bg thread sends the configured surface (or an error
    /// string) here when `surface.configure(...)` returns.
    pub rx: std::sync::mpsc::Receiver<Result<wgpu::Surface<'static>, String>>,
    pub started_at: std::time::Instant,
}
// --- end weezterm remote features ---

#[derive(Clone, Copy)]
pub struct RawHandlePair {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

// `RawWindowHandle` and `RawDisplayHandle` are plain enum-of-values
// types (HWND as NonZeroIsize on Win32, etc.). They are safe to
// transfer between threads — we use this so the bg
// `wgpu-async-configure` thread can rebuild a fresh `wgpu::Surface`
// from the window's HWND after dropping the previous one.
unsafe impl Send for RawHandlePair {}
unsafe impl Sync for RawHandlePair {}

impl RawHandlePair {
    fn new(window: &Window) -> Self {
        Self {
            window: window.window_handle().expect("window handle").as_raw(),
            display: window.display_handle().expect("display handle").as_raw(),
        }
    }
}

impl HasWindowHandle for RawHandlePair {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        unsafe { Ok(WindowHandle::borrow_raw(self.window)) }
    }
}

impl HasDisplayHandle for RawHandlePair {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        unsafe { Ok(DisplayHandle::borrow_raw(self.display)) }
    }
}

pub struct WebGpuTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    queue: Arc<wgpu::Queue>,
}

impl std::ops::Deref for WebGpuTexture {
    type Target = wgpu::Texture;
    fn deref(&self) -> &Self::Target {
        &self.texture
    }
}

impl Texture2d for WebGpuTexture {
    fn write(&self, rect: Rect, im: &dyn BitmapImage) {
        let (im_width, im_height) = im.image_dimensions();

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min_x() as u32,
                    y: rect.min_y() as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            im.pixel_data_slice(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(im_width as u32 * 4),
                rows_per_image: Some(im_height as u32),
            },
            wgpu::Extent3d {
                width: im_width as u32,
                height: im_height as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    fn read(&self, _rect: Rect, _im: &mut dyn BitmapImage) {
        unimplemented!();
    }

    fn width(&self) -> usize {
        self.width as usize
    }

    fn height(&self) -> usize {
        self.height as usize
    }
}

impl WebGpuTexture {
    pub fn new(width: u32, height: u32, state: &WebGpuState) -> anyhow::Result<Self> {
        let limit = state.device.limits().max_texture_dimension_2d;

        if width > limit || height > limit {
            // Ideally, wgpu would have a fallible create_texture method,
            // but it doesn't: instead it will panic if the requested
            // dimension is too large.
            // So we check the limit ourselves here.
            // <https://github.com/wezterm/wezterm/issues/3713>
            anyhow::bail!(
                "texture dimensions {width}x{height} exceeed the \
                 max dimension {limit} supported by your GPU"
            );
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let view_formats = if state
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format, format.remove_srgb_suffix()]
        } else {
            vec![]
        };
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("Texture Atlas"),
            view_formats: &view_formats,
        });
        Ok(Self {
            texture,
            width,
            height,
            queue: Arc::clone(&state.queue),
        })
    }
}

pub fn adapter_info_to_gpu_info(info: wgpu::AdapterInfo) -> GpuInfo {
    GpuInfo {
        name: info.name,
        vendor: Some(info.vendor),
        device: Some(info.device),
        device_type: format!("{:?}", info.device_type),
        driver: if info.driver.is_empty() {
            None
        } else {
            Some(info.driver)
        },
        driver_info: if info.driver_info.is_empty() {
            None
        } else {
            Some(info.driver_info)
        },
        backend: format!("{:?}", info.backend),
    }
}

fn compute_compatibility_list(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    surface: &wgpu::Surface,
) -> Vec<String> {
    smol::block_on(async {
        let mut out = Vec::new();
        for a in instance.enumerate_adapters(backends).await {
            let info = adapter_info_to_gpu_info(a.get_info());
            let compatible = a.is_surface_supported(&surface);
            out.push(format!(
                "{}, compatible={}",
                info.to_string(),
                if compatible { "yes" } else { "NO" }
            ));
        }
        out
    })
}

// --- weezterm remote features ---
/// Returns `true` when the user's config asks the window to be
/// translucent — either via `window_background_opacity < 1.0` or via
/// `win32_system_backdrop` requesting Mica/Acrylic/Tabbed. `Auto` and
/// `Disable` do not by themselves indicate translucency; opacity is
/// the dominant signal in that case.
///
/// This is consulted at surface configuration time to decide whether
/// Mode A should request a `PreMultiplied` alpha mode (transparent
/// composition) or `Opaque` (zero-alpha-cost composition).
fn window_uses_translucency(config: &ConfigHandle) -> bool {
    use config::SystemBackdrop;
    if config.window_background_opacity < 1.0 {
        return true;
    }
    matches!(
        config.win32_system_backdrop,
        SystemBackdrop::Acrylic | SystemBackdrop::Mica | SystemBackdrop::Tabbed
    )
}
// --- end weezterm remote features ---

impl WebGpuState {
    pub async fn new(
        window: &Window,
        dimensions: Dimensions,
        config: &ConfigHandle,
        // --- weezterm remote features ---
        mode: ::window::render_mode::RenderMode,
        // --- end weezterm remote features ---
    ) -> anyhow::Result<Self> {
        let handle = RawHandlePair::new(window);
        Self::new_impl(handle, dimensions, config, mode).await
    }

    pub async fn new_impl(
        handle: RawHandlePair,
        dimensions: Dimensions,
        config: &ConfigHandle,
        // --- weezterm remote features ---
        mode: ::window::render_mode::RenderMode,
        // --- end weezterm remote features ---
    ) -> anyhow::Result<Self> {
        let backends = wgpu::Backends::all();
        // --- weezterm remote features ---
        // Phase 2b: explicitly configure the DX12 backend options based
        // on the chosen render mode. Mode A (DComp) uses
        // DxgiFromVisual + waitable so we get transparent composition
        // and Present-pacing without GetMessage waits. Mode B
        // (Classic) preserves the historical HWND-direct swapchain
        // behaviour from pre-wgpu-28: HWND swapchain, no waitable
        // (Wait was added as a default in v28; we opt out to avoid
        // behaviour drift). On non-Windows platforms the dx12 options
        // are inert. See docs/windows-rendering-design.md §4.
        let backend_options = wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                presentation_system: match mode {
                    ::window::render_mode::RenderMode::WgpuDComp => {
                        wgpu::Dx12SwapchainKind::DxgiFromVisual
                    }
                    _ => wgpu::Dx12SwapchainKind::DxgiFromHwnd,
                },
                latency_waitable_object: match mode {
                    ::window::render_mode::RenderMode::WgpuDComp => {
                        wgpu::Dx12UseFrameLatencyWaitableObject::Wait
                    }
                    _ => wgpu::Dx12UseFrameLatencyWaitableObject::None,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            backend_options,
            ..Default::default()
        });
        // --- end weezterm remote features ---
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&handle)?)?
        };

        let mut adapter: Option<wgpu::Adapter> = None;

        if let Some(preference) = &config.webgpu_preferred_adapter {
            for a in instance.enumerate_adapters(backends).await {
                if !a.is_surface_supported(&surface) {
                    let info = adapter_info_to_gpu_info(a.get_info());
                    log::warn!("{} is not compatible with surface", info.to_string());
                    continue;
                }

                let info = a.get_info();

                if preference.name != info.name {
                    continue;
                }

                if preference.device_type != format!("{:?}", info.device_type) {
                    continue;
                }

                if preference.backend != format!("{:?}", info.backend) {
                    continue;
                }

                if let Some(driver) = &preference.driver {
                    if *driver != info.driver {
                        continue;
                    }
                }
                if let Some(vendor) = &preference.vendor {
                    if *vendor != info.vendor {
                        continue;
                    }
                }
                if let Some(device) = &preference.device {
                    if *device != info.device {
                        continue;
                    }
                }

                adapter.replace(a);
                break;
            }

            if adapter.is_none() {
                let adapters = compute_compatibility_list(&instance, backends, &surface);
                log::warn!(
                    "Your webgpu preferred adapter '{}' was either not \
                     found or is not compatible with your display. Available:\n{}",
                    preference.to_string(),
                    adapters.join("\n")
                );
            }
        }

        if adapter.is_none() {
            adapter = Some(
                instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: match config.webgpu_power_preference {
                            WebGpuPowerPreference::HighPerformance => {
                                wgpu::PowerPreference::HighPerformance
                            }
                            WebGpuPowerPreference::LowPower => wgpu::PowerPreference::LowPower,
                        },
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: config.webgpu_force_fallback_adapter,
                    })
                    .await?,
            );
        }

        let adapter = adapter.ok_or_else(|| {
            let adapters = compute_compatibility_list(&instance, backends, &surface);
            anyhow!(
                "no compatible adapter found. Available:\n{}",
                adapters.join("\n")
            )
        })?;

        let adapter_info = adapter.get_info();
        log::trace!("Using adapter: {adapter_info:?}");
        let caps = surface.get_capabilities(&adapter);
        log::trace!("caps: {caps:?}");
        let downlevel_caps = adapter.get_downlevel_capabilities();
        log::trace!("downlevel_caps: {downlevel_caps:?}");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::downlevel_defaults()
                }
                .using_resolution(adapter.limits()),
                label: None,
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let queue = Arc::new(queue);

        // --- weezterm remote features ---
        // Phase 2c: Mode A wants `PreMultiplied` when the user has
        // requested any translucency (so DComp blends the swapchain
        // output with whatever's behind the window — Mica/Acrylic
        // backdrop, desktop, other windows). When the window is fully
        // opaque we ask for `Opaque` so DXGI/DComp can skip per-pixel
        // alpha. Mode B (HWND swapchain) is necessarily opaque
        // regardless of `alpha_mode` (DXGI HWND swapchains only
        // support `DXGI_ALPHA_MODE_IGNORE`), so preserve the
        // historical caps-driven fallback there to avoid breaking
        // existing behaviour.
        let translucent = window_uses_translucency(config);
        // --- end weezterm remote features ---

        // Explicitly request an SRGB format, if available
        let pref_format_srgb = caps.formats[0].add_srgb_suffix();
        let format = if caps.formats.contains(&pref_format_srgb) {
            pref_format_srgb
        } else {
            caps.formats[0]
        };

        // Need to check that this is supported, as trying to set
        // view_formats without it will cause surface.configure
        // to panic
        // <https://github.com/wezterm/wezterm/issues/3565>
        let view_formats = if downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format.add_srgb_suffix(), format.remove_srgb_suffix()]
        } else {
            vec![]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: dimensions.pixel_width as u32,
            height: dimensions.pixel_height as u32,
            // --- weezterm remote features ---
            // Prefer Mailbox (latest-image, no queue) over Fifo on
            // RDP/WARP. Fifo's vsync wait can stall presents to ~1Hz on
            // virtual GPUs, leaving DWM displaying a stale (stretched)
            // backbuffer for many seconds after a resize. Mailbox
            // returns the most recent submitted frame as the front
            // buffer with no queue, so post-resize redraws appear
            // immediately. Fall through to AutoVsync (≈ Fifo) only if
            // neither Mailbox nor Immediate is supported.
            present_mode: {
                let chosen = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                    wgpu::PresentMode::Mailbox
                } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
                    wgpu::PresentMode::Immediate
                } else {
                    wgpu::PresentMode::AutoVsync
                };
                log::info!(
                    "[render] surface present_modes={:?} chose={:?}",
                    caps.present_modes,
                    chosen,
                );
                chosen
            },
            // --- end weezterm remote features ---
            // --- weezterm remote features ---
            alpha_mode: match mode {
                ::window::render_mode::RenderMode::WgpuDComp if translucent => {
                    wgpu::CompositeAlphaMode::PreMultiplied
                }
                ::window::render_mode::RenderMode::WgpuDComp => wgpu::CompositeAlphaMode::Opaque,
                _ => {
                    if caps
                        .alpha_modes
                        .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
                    {
                        wgpu::CompositeAlphaMode::PostMultiplied
                    } else if caps
                        .alpha_modes
                        .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
                    {
                        wgpu::CompositeAlphaMode::PreMultiplied
                    } else {
                        wgpu::CompositeAlphaMode::Auto
                    }
                }
            },
            // --- end weezterm remote features ---
            view_formats,
            // --- weezterm remote features ---
            // Phase 2b: Mode A uses frame-latency=1 (paired with the
            // waitable handle) to get latest-image pacing without
            // queueing extra frames. Mode B preserves the
            // historical value of 2.
            desired_maximum_frame_latency: match mode {
                ::window::render_mode::RenderMode::WgpuDComp => 1,
                _ => 2,
            },
            // --- end weezterm remote features ---
        };
        surface.configure(&device, &config);

        // --- weezterm remote features ---
        // Phase 3: HAL-level hooks for Mode A. wgpu's API exposes
        // `desired_maximum_frame_latency` (set above to 1) and
        // `latency_waitable_object: Wait` (set in the BackendOptions
        // above) and wgpu-hal already calls `SetMaximumFrameLatency`
        // and `WaitForSingleObject` internally during
        // `acquire_texture`. We *additionally* drop down to the raw
        // `IDXGISwapChain3` here to:
        //
        //   1. Belt-and-braces re-call `SetMaximumFrameLatency(1)`
        //      via the public `IDXGISwapChain2` method (idempotent).
        //   2. Fetch our own copy of the waitable HANDLE via
        //      `GetFrameLatencyWaitableObject` so the render loop
        //      can wait on it *before* `surface.get_current_texture`
        //      (which is where wgpu-hal performs its own wait). This
        //      gives us the chance to discard wrong-size frames or
        //      skip work when the GPU is still busy.
        //
        // Per Microsoft docs, each call to
        // `GetFrameLatencyWaitableObject` increments the swap chain's
        // internal refcount on the waitable; we must therefore call
        // `CloseHandle` in `Drop`. wgpu-hal owns its own separate
        // HANDLE which it closes via `release_resources`, so the two
        // do not interfere.
        //
        // The optional `DXGI_SCALING_NONE` swap-chain rebuild
        // mentioned in `docs/windows-rendering-design.md` §6 Phase 3
        // is intentionally NOT done here — it would require manually
        // recreating the swap chain through the HAL and is too risky
        // for the marginal artifact-reduction benefit (DComp +
        // waitable already absorb most of the resize stretch).
        // Tracked in `docs/upstream-wgpu-pr-notes.md`.
        #[cfg(windows)]
        let frame_latency_waitable: Option<winapi::shared::ntdef::HANDLE> =
            if mode == ::window::render_mode::RenderMode::WgpuDComp {
                let mut waitable: Option<winapi::shared::ntdef::HANDLE> = None;
                unsafe {
                    if let Some(raw_surface) = surface.as_hal::<wgpu::hal::api::Dx12>() {
                        // `swap_chain()` returns
                        // `Option<windows::Win32::Graphics::Dxgi::IDXGISwapChain3>`
                        // (from wgpu-hal's `windows = "0.62"` dep).
                        // `IDXGISwapChain3: Deref<Target =
                        // IDXGISwapChain2>` so the `IDXGISwapChain2`
                        // methods are inherent.
                        if let Some(sc) = raw_surface.swap_chain() {
                            // Belt-and-braces; wgpu-hal already does this.
                            if let Err(e) = sc.SetMaximumFrameLatency(1) {
                                log::warn!(
                                    "[render] HAL SetMaximumFrameLatency(1) \
                                     failed: {e:?}"
                                );
                            }
                            // Get our own HANDLE; closed in `Drop`.
                            let h = sc.GetFrameLatencyWaitableObject();
                            if !h.0.is_null() {
                                // `windows::Foundation::HANDLE.0` is
                                // `*mut core::ffi::c_void`; winapi's
                                // `HANDLE` is `*mut winapi::ctypes::c_void`.
                                // Both are `*mut c_void` ABI-wise, so
                                // an `as` cast is safe.
                                waitable = Some(h.0 as winapi::shared::ntdef::HANDLE);
                                log::info!(
                                    "[render] HAL frame-latency waitable \
                                     acquired (mode={})",
                                    mode.as_str()
                                );
                            } else {
                                log::warn!(
                                    "[render] HAL GetFrameLatencyWaitableObject \
                                     returned null; waitable disabled"
                                );
                            }
                        } else {
                            log::warn!(
                                "[render] HAL swap_chain accessor returned None; \
                                 waitable disabled (frame latency relies on \
                                 wgpu-API config only)"
                            );
                        }
                    } else {
                        log::warn!(
                            "[render] surface.as_hal::<Dx12>() returned None; \
                             not running on the DX12 backend? Waitable disabled"
                        );
                    }
                }
                waitable
            } else {
                None
            };
        // --- end weezterm remote features ---

        // --- weezterm remote features ---
        // Phase 2c diagnostic line. Format mirrors the `[render] mode=...`
        // startup line and is grepped by `tests/ux/test_transparency.py`.
        // The fields are stable; do not rename without updating the test.
        log::info!(
            "[render] surface mode={} alpha_mode={:?} translucent={} \
             frame_latency={} present_mode={:?} format={:?}",
            mode.as_str(),
            config.alpha_mode,
            translucent,
            config.desired_maximum_frame_latency,
            config.present_mode,
            config.format,
        );
        // --- end weezterm remote features ---

        let shader = device.create_shader_module(wgpu::include_wgsl!("../shader.wgsl"));

        let shader_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("ShaderUniform bind group layout"),
            });

        let texture_nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let texture_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture bind group layout"),
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    &shader_uniform_bind_group_layout,
                    &texture_bind_group_layout,
                    &texture_bind_group_layout,
                ],
                immediate_size: 0,
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),

            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            adapter_info,
            downlevel_caps,
            surface: RefCell::new(Some(surface)),
            device,
            queue,
            config: RefCell::new(config),
            dimensions: RefCell::new(dimensions),
            render_pipeline,
            handle,
            shader_uniform_bind_group_layout,
            texture_bind_group_layout,
            texture_nearest_sampler,
            texture_linear_sampler,
            // --- weezterm remote features ---
            instance,
            mode,
            #[cfg(windows)]
            frame_latency_waitable,
            pending_resize: RefCell::new(None),
            last_resize_request: RefCell::new(None),
            live_resize_active: RefCell::new(false),
            pending_configure: RefCell::new(None),
            queued_configure_dims: RefCell::new(None),
            // --- end weezterm remote features ---
        })
    }

    pub fn create_uniform(&self, uniform: ShaderUniform) -> wgpu::BindGroup {
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ShaderUniform Buffer"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.shader_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
            label: Some("ShaderUniform Bind Group"),
        })
    }

    #[allow(unused_mut)]
    pub fn resize(&self, mut dims: Dimensions) {
        // During a live resize on Windows, the Dimensions that we're processing may be
        // lagging behind the true client size. We have to take the very latest value
        // from the window or else the underlying driver will raise an error about
        // the mismatch, so we need to sneakily read through the handle
        match self.handle.window {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => {
                let mut rect = unsafe { std::mem::zeroed() };
                unsafe { winapi::um::winuser::GetClientRect(h.hwnd.get() as _, &mut rect) };
                dims.pixel_width = (rect.right - rect.left) as usize;
                dims.pixel_height = (rect.bottom - rect.top) as usize;
            }
            _ => {}
        }

        if dims == *self.dimensions.borrow() {
            return;
        }
        *self.dimensions.borrow_mut() = dims;
        // --- weezterm remote features ---
        // Defer the actual `surface.configure` (which calls
        // DXGI ResizeBuffers — 100-1700ms on a virtual GPU) to the
        // render path. Many WM_SIZE events during a live drag will
        // each call resize(), but they coalesce to ONE configure
        // per "settled" size, executed by `apply_pending_resize`
        // immediately before `get_current_texture`.
        *self.pending_resize.borrow_mut() = Some(dims);
        *self.last_resize_request.borrow_mut() = Some(std::time::Instant::now());
        // --- end weezterm remote features ---
    }

    // --- weezterm remote features ---
    /// Inform the GPU surface that an interactive resize drag has
    /// started or ended. While `active` is `true`, `apply_pending_resize`
    /// will defer all configures so the user can drag the window edge
    /// smoothly without blocking on per-step `ResizeBuffers` calls
    /// (which cost 600-1700ms on the WARP driver under RDP). When the
    /// drag ends, the next paint pays the configure cost ONCE for the
    /// final dims.
    pub fn set_live_resize_active(&self, active: bool) {
        *self.live_resize_active.borrow_mut() = active;
    }

    /// Returns the current value of the `live_resize_active` flag.
    /// Used by `TermWindow::resize` to detect a live -> idle transition
    /// and force a re-paint when dims didn't change.
    pub fn is_live_resize_active(&self) -> bool {
        *self.live_resize_active.borrow()
    }

    /// Apply any pending deferred resize. Call immediately before
    /// `surface.get_current_texture()` in the render path.
    ///
    /// Returns `true` if the surface configuration was changed (the
    /// caller may want to invalidate caches).
    ///
    /// **Async configure model.** The actual `surface.configure(...)`
    /// call invokes `IDXGIFactory::CreateSwapChainForHwnd`, which on
    /// WARP/RDP virtual GPUs has been measured at **3.6 seconds**.
    /// Running it on the GUI thread blocks every smol task, the
    /// codec read pump in particular, and the remote
    /// `wezterm-mux-server` drops us with `os error 10054`. Instead
    /// we:
    ///
    /// 1. Drain any in-flight `pending_configure` here. If the bg
    ///    thread has finished, atomically swap the new surface in
    ///    and update `self.config` to match. The OLD surface is
    ///    moved to *another* bg thread for drop (also slow on WARP).
    /// 2. Decide what dims we want next, preferring the most-recent
    ///    `pending_resize`, then any `queued_configure_dims`, then
    ///    the live `GetClientRect` (Windows only, as a fallback for
    ///    OS-driven rect changes that didn't go through our event
    ///    path).
    /// 3. If a configure is already in flight for those exact dims,
    ///    do nothing. If for *different* dims, queue the new dims
    ///    in `queued_configure_dims` so we kick off another configure
    ///    when the current one lands. If no configure is in flight,
    ///    spawn one now.
    ///
    /// While `live_resize_active` is true (interactive drag), we do
    /// NOT start a new configure — DWM stretches the existing
    /// swapchain image to fill the new client rect, which is much
    /// less jarring on a slow GPU than churning configures per
    /// pixel of drag.
    pub fn apply_pending_resize(&self) -> bool {
        let live = *self.live_resize_active.borrow();

        // ---------------------------------------------------------
        // STEP 1: drain any ready bg-configured surface.
        // We do this even during a live drag — if a configure
        // happened to land, we want to swap it in. The next live
        // WM_SIZE will cause us to queue another.
        // ---------------------------------------------------------
        let drained: Option<
            Result<(wgpu::Surface<'static>, Dimensions, std::time::Duration), String>,
        > = {
            let mut pc_borrow = self.pending_configure.borrow_mut();
            match pc_borrow.as_mut() {
                None => None,
                Some(pc) => match pc.rx.try_recv() {
                    Ok(Ok(new_surface)) => {
                        let target = pc.target_dims;
                        let elapsed = pc.started_at.elapsed();
                        *pc_borrow = None;
                        Some(Ok((new_surface, target, elapsed)))
                    }
                    Ok(Err(e)) => {
                        *pc_borrow = None;
                        Some(Err(e))
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        *pc_borrow = None;
                        Some(Err("async-configure thread disconnected".into()))
                    }
                },
            }
        };

        let mut configured_change = false;
        match drained {
            Some(Ok((new_surface, target, elapsed))) => {
                let (old_w, old_h) = {
                    let cfg = self.config.borrow();
                    (cfg.width, cfg.height)
                };
                let new_w = target.pixel_width as u32;
                let new_h = target.pixel_height as u32;
                {
                    let mut config = self.config.borrow_mut();
                    config.width = new_w;
                    config.height = new_h;
                }
                // The OLD surface was already moved to the bg thread
                // (which dropped it before creating + configuring the
                // new one), so `self.surface` is currently `None`. Put
                // the freshly-configured surface in.
                *self.surface.borrow_mut() = Some(new_surface);
                log::info!(
                    "[render] webgpu async configure landed {}x{} -> {}x{} bg_elapsed={:?}",
                    old_w,
                    old_h,
                    new_w,
                    new_h,
                    elapsed,
                );
                configured_change = true;
            }
            Some(Err(e)) => {
                log::warn!("[render] webgpu async configure failed: {}", e);
            }
            None => {}
        }

        // ---------------------------------------------------------
        // STEP 2: figure out the size we WANT next.
        // ---------------------------------------------------------
        let pending_now = *self.pending_resize.borrow();
        let queued_now = *self.queued_configure_dims.borrow();
        #[allow(unused_variables)]
        let target_dims = match (pending_now, queued_now, self.handle.window) {
            (Some(d), _, _) => Some(d),
            (None, Some(d), _) => Some(d),
            #[cfg(windows)]
            (None, None, RawWindowHandle::Win32(h)) if !live => {
                let mut rect = unsafe { std::mem::zeroed() };
                unsafe { winapi::um::winuser::GetClientRect(h.hwnd.get() as _, &mut rect) };
                let w = (rect.right - rect.left) as usize;
                let hh = (rect.bottom - rect.top) as usize;
                if w > 0 && hh > 0 {
                    Some(Dimensions {
                        pixel_width: w,
                        pixel_height: hh,
                        dpi: 0,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(dims) = target_dims else {
            return configured_change;
        };

        if live {
            // Don't start a new configure during interactive drag.
            // Stale stretched frame is preferable to per-pixel
            // configure churn on a slow GPU.
            return configured_change;
        }

        let new_w = dims.pixel_width as u32;
        let new_h = dims.pixel_height as u32;
        if new_w == 0 || new_h == 0 {
            return configured_change;
        }

        // If the active surface is already at the target, clear
        // pending and we're done.
        let (cur_w, cur_h) = {
            let cfg = self.config.borrow();
            (cfg.width, cfg.height)
        };
        if cur_w == new_w && cur_h == new_h {
            self.pending_resize.borrow_mut().take();
            self.queued_configure_dims.borrow_mut().take();
            return configured_change;
        }

        // ---------------------------------------------------------
        // STEP 3: is a configure already in flight?
        // ---------------------------------------------------------
        let in_flight_target = self
            .pending_configure
            .borrow()
            .as_ref()
            .map(|pc| pc.target_dims);
        if let Some(t) = in_flight_target {
            let t_w = t.pixel_width as u32;
            let t_h = t.pixel_height as u32;
            if t_w == new_w && t_h == new_h {
                // Already configuring exactly these dims; just wait.
                self.pending_resize.borrow_mut().take();
                self.queued_configure_dims.borrow_mut().take();
                return configured_change;
            } else {
                // In-flight is for stale dims. Queue the latest;
                // we'll start a new configure when the current lands.
                *self.queued_configure_dims.borrow_mut() = Some(dims);
                self.pending_resize.borrow_mut().take();
                return configured_change;
            }
        }

        // ---------------------------------------------------------
        // STEP 4: spawn a bg configure thread.
        //
        // CRITICAL ordering: DXGI does NOT allow two swap chains for
        // the same HWND simultaneously. The bg thread therefore must:
        //   1. Drop the OLD surface FIRST (which destroys the OLD
        //      swap chain — can take up to 11 s on WARP/RDP because
        //      `wait_for_present_queue_idle` waits on the stale
        //      present queue — but we're on a bg thread so the GUI
        //      stays interactive)
        //   2. Build a fresh `SurfaceTargetUnsafe` from the window
        //      handle and `instance.create_surface_unsafe(...)` —
        //      both fast, no swap chain yet
        //   3. `surface.configure(device, cfg)` — this is what
        //      actually invokes `CreateSwapChainForHwnd`. Now the
        //      OLD swap chain is gone, so DXGI accepts it.
        // ---------------------------------------------------------
        self.pending_resize.borrow_mut().take();
        self.queued_configure_dims.borrow_mut().take();

        // Take ownership of the OLD surface so the bg thread can
        // drop it before creating the new one. `self.surface` is
        // `None` until the bg thread completes and the next paint
        // drains the channel; the render path's `has_pending_configure()`
        // gate means `get_current_texture()` is never called during
        // this window.
        let old_surface = self.surface.borrow_mut().take();

        // Build the SurfaceConfiguration the bg thread will use.
        // We clone the active config and patch in the new dims so
        // format / present_mode / alpha_mode / view_formats / etc.
        // all carry over.
        let mut bg_config = self.config.borrow().clone();
        bg_config.width = new_w;
        bg_config.height = new_h;

        let device = self.device.clone();
        let instance = self.instance.clone();
        let handle = self.handle;
        let (tx, rx) = std::sync::mpsc::channel();
        let started_at = std::time::Instant::now();
        let bg_dims = dims;

        log::info!(
            "[render] queueing async webgpu configure {}x{} -> {}x{}",
            cur_w,
            cur_h,
            new_w,
            new_h,
        );

        // Capture the HWND value (as usize so it's Send) so the bg
        // thread can wake the GUI with InvalidateRect after configure
        // completes. Otherwise the GUI thread would only drain the
        // channel on the next cursor-blink tick (~500 ms latency
        // after the bg thread has already finished).
        #[cfg(windows)]
        let wake_hwnd: usize = match self.handle.window {
            RawWindowHandle::Win32(h) => h.hwnd.get() as usize,
            _ => 0,
        };
        #[cfg(not(windows))]
        let wake_hwnd: usize = 0;

        let spawn_res = std::thread::Builder::new()
            .name("wgpu-async-configure".into())
            .spawn(move || {
                let bg_t = std::time::Instant::now();
                // Install a panic hook that captures the panic
                // payload + location for this thread only, so
                // catch_unwind below can report a meaningful error.
                let captured: std::sync::Arc<std::sync::Mutex<Option<String>>> =
                    std::sync::Arc::new(std::sync::Mutex::new(None));
                let captured_for_hook = captured.clone();
                let prev_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    let msg = info
                        .payload()
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| info.payload().downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string panic>".to_string());
                    let loc = info
                        .location()
                        .map(|l| format!(" at {}:{}", l.file(), l.line()))
                        .unwrap_or_default();
                    *captured_for_hook.lock().unwrap() = Some(format!("{msg}{loc}"));
                }));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Step 1: drop the OLD surface BEFORE creating
                    // the new swap chain. DXGI rejects two swap
                    // chains on the same HWND.
                    let drop_t = std::time::Instant::now();
                    drop(old_surface);
                    let drop_elapsed = drop_t.elapsed();

                    // Step 2: build a fresh SurfaceTargetUnsafe and
                    // a Surface object (no swap chain yet).
                    let raw_target = unsafe {
                        wgpu::SurfaceTargetUnsafe::from_window(&handle)
                            .expect("SurfaceTargetUnsafe::from_window failed in bg thread")
                    };
                    let new_surface = unsafe {
                        instance
                            .create_surface_unsafe(raw_target)
                            .expect("instance.create_surface_unsafe failed in bg thread")
                    };

                    // Step 3: configure — this invokes
                    // CreateSwapChainForHwnd.
                    let cfg_t = std::time::Instant::now();
                    new_surface.configure(&device, &bg_config);
                    let cfg_elapsed = cfg_t.elapsed();
                    log::info!(
                        "[render] async webgpu configure {}x{}: drop_old={:?} configure={:?}",
                        bg_dims.pixel_width,
                        bg_dims.pixel_height,
                        drop_elapsed,
                        cfg_elapsed,
                    );
                    new_surface
                }))
                .map_err(|_| {
                    captured
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap_or_else(|| "wgpu surface.configure panicked".to_string())
                });
                std::panic::set_hook(prev_hook);
                log::info!(
                    "[render] async webgpu configure for {}x{} bg-thread total time={:?}",
                    bg_dims.pixel_width,
                    bg_dims.pixel_height,
                    bg_t.elapsed(),
                );
                let _ = tx.send(result);
                // Wake the GUI thread so it drains the channel
                // immediately and swaps the new surface in.
                // InvalidateRect is documented thread-safe.
                #[cfg(windows)]
                if wake_hwnd != 0 {
                    unsafe {
                        winapi::um::winuser::InvalidateRect(
                            wake_hwnd as winapi::shared::windef::HWND,
                            std::ptr::null(),
                            0,
                        );
                    }
                }
            });

        if let Err(e) = spawn_res {
            // Couldn't spawn the bg thread; we already took the OLD
            // surface out of self.surface. Recover by doing the whole
            // operation inline on the GUI thread (yes, this blocks,
            // but spawn failures are exceptionally rare).
            log::error!(
                "[render] failed to spawn async-configure thread: {e}; \
                 falling back to inline drop+create+configure"
            );
            // The OLD surface was already moved to a local; drop it
            // to free the swap chain slot for the HWND.
            // (No `old_surface` binding here because it was moved
            // into the closure — but the closure didn't run, so
            // `old_surface` was dropped at the end of `spawn_res`'s
            // expression. Nothing more to do here.)
            let raw_target = match unsafe { wgpu::SurfaceTargetUnsafe::from_window(&self.handle) } {
                Ok(t) => t,
                Err(e) => {
                    log::error!("[render] inline SurfaceTargetUnsafe::from_window failed: {e}");
                    return configured_change;
                }
            };
            let new_surface = match unsafe { self.instance.create_surface_unsafe(raw_target) } {
                Ok(s) => s,
                Err(e) => {
                    log::error!("[render] inline instance.create_surface_unsafe failed: {e}");
                    return configured_change;
                }
            };
            {
                let mut config = self.config.borrow_mut();
                config.width = new_w;
                config.height = new_h;
            }
            let cfg = self.config.borrow().clone();
            new_surface.configure(&self.device, &cfg);
            *self.surface.borrow_mut() = Some(new_surface);
            return true;
        }

        *self.pending_configure.borrow_mut() = Some(PendingConfigure {
            target_dims: dims,
            rx,
            started_at,
        });

        configured_change
    }

    /// Returns true while a background `surface.configure(...)` is
    /// in flight. The render path uses this to suppress the
    /// "wrong-size-frame discard" InvalidateRect loop — during the
    /// async configure the active surface's backing swap chain is
    /// at the OLD size, so by definition `cfg.{w,h} != client size`,
    /// and dropping the frame would just spin WM_PAINT until the
    /// configure lands (potentially seconds on WARP/RDP). DWM
    /// happily stretches the old image in the meantime.
    pub fn has_pending_configure(&self) -> bool {
        self.pending_configure.borrow().is_some()
    }
    // --- end weezterm remote features ---
}

// --- weezterm remote features ---
// Phase 3: release the waitable HANDLE we acquired via
// `IDXGISwapChain2::GetFrameLatencyWaitableObject`. wgpu-hal owns its
// own separate HANDLE which it releases when the swap chain is
// destroyed (see `wgpu_hal::dx12::SwapChain::release_resources`); we
// only close the additional handle reference returned by our own
// call. Closing the wrong handle would corrupt the kernel handle
// table for this process, hence the explicit cfg gate and the
// take()-based ownership transfer.
impl Drop for WebGpuState {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(h) = self.frame_latency_waitable.take() {
            unsafe {
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
}
// --- end weezterm remote features ---
