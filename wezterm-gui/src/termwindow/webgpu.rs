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
    pub surface: wgpu::Surface<'static>,
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
    // --- end weezterm remote features ---
}

pub struct RawHandlePair {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

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
            present_mode: wgpu::PresentMode::Fifo,
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
            surface,
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
            mode,
            #[cfg(windows)]
            frame_latency_waitable,
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
        let mut config = self.config.borrow_mut();
        config.width = dims.pixel_width as u32;
        config.height = dims.pixel_height as u32;
        if config.width > 0 && config.height > 0 {
            // Avoid reconfiguring with a 0 sized surface, as webgpu will
            // panic in that case
            // <https://github.com/wezterm/wezterm/issues/2881>
            self.surface.configure(&self.device, &config);
        }
    }
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
