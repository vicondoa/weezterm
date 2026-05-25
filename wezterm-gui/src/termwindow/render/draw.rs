use crate::colorease::ColorEaseUniform;
use crate::termwindow::webgpu::ShaderUniform;
use crate::termwindow::RenderFrame;
use crate::uniforms::UniformBuilder;
use ::window::glium;
use ::window::glium::uniforms::{
    MagnifySamplerFilter, MinifySamplerFilter, Sampler, SamplerWrapFunction,
};
use ::window::glium::{BlendingFunction, LinearBlendingFactor, Surface};
use config::FreeTypeLoadTarget;

impl crate::TermWindow {
    pub fn call_draw(&mut self, frame: &mut RenderFrame) -> anyhow::Result<()> {
        match frame {
            RenderFrame::Glium(ref mut frame) => self.call_draw_glium(frame),
            RenderFrame::WebGpu => self.call_draw_webgpu(),
        }
    }

    fn call_draw_webgpu(&mut self) -> anyhow::Result<()> {
        use crate::termwindow::webgpu::WebGpuTexture;

        let webgpu = self.backend.webgpu().unwrap().clone();
        let render_state = self.render_state.as_ref().unwrap();

        // --- weezterm remote features ---
        // Phase 3: wait on the frame-latency waitable object before
        // recording the next frame. This is the HAL counterpart to
        // wgpu's internal wait inside `surface.get_current_texture()`
        // — placing it here lets us discard a wrong-size frame (next
        // block) cheaply if the GPU advanced between WM_PAINT and
        // now. 100 ms timeout chosen to match wgpu-hal's own default
        // wait timeout. WAIT_TIMEOUT (and WAIT_FAILED) are tolerated
        // — we just proceed and let the subsequent
        // `get_current_texture` either succeed or report Lost/Outdated
        // for the outer error handler to deal with.
        #[cfg(windows)]
        if let Some(h) = webgpu.frame_latency_waitable {
            unsafe {
                winapi::um::synchapi::WaitForSingleObjectEx(h, 100, 0);
            }
        }
        // --- end weezterm remote features ---

        // --- weezterm remote features ---
        // Apply any deferred surface.configure that was coalesced
        // from the resize event(s). On a virtual GPU this can take
        // 100-800ms, but we now pay it at most once per paint
        // instead of once per WM_SIZE during a live drag. By the
        // time we get here, `apply_dimensions` has already updated
        // the terminal grid for these dims, so the very next paint
        // will be visually correct (no "snap to original size in
        // big window" middle frame).
        let _config_changed = webgpu.apply_pending_resize();
        // --- end weezterm remote features ---

        // --- weezterm remote features ---
        // While a background `surface.configure(...)` is in flight,
        // the active swap chain is intentionally still at the OLD
        // pixel size. `self.dimensions` and `self.terminal_size`
        // have ALREADY been updated to the NEW size by
        // `apply_dimensions`, so if we proceed to render now we will
        // project NEW pixel coordinates into an OLD-size texture —
        // quads beyond the OLD bounds get clipped, and DWM stretches
        // the resulting fragment to fill the NEW client rect. The
        // user sees a disorienting "stretched fragment of new
        // content" for the entire duration of the bg configure
        // (~3.7 s on WARP/RDP).
        //
        // Skip the rest of the draw entirely. The previous frame
        // (rendered correctly at OLD pixel coords with OLD content
        // into the OLD-size surface) stays on the swap chain. DWM
        // stretches that coherent OLD frame to the NEW client rect,
        // which looks like a clean zoom — the same UX users had
        // with the old synchronous-configure path. The bg thread
        // itself calls `InvalidateRect` when it finishes (see
        // `webgpu.rs::apply_pending_resize`), so the GUI thread
        // reliably wakes up to drain the channel and swap the new
        // surface in.
        if webgpu.has_pending_configure() {
            log::debug!(
                "[render] skipping paint while async configure in flight \
                 (cfg={}x{} self.dims={}x{})",
                webgpu.config.borrow().width,
                webgpu.config.borrow().height,
                self.dimensions.pixel_width,
                self.dimensions.pixel_height,
            );
            return Ok(());
        }
        // --- end weezterm remote features ---

        // --- weezterm remote features ---
        // Phase 5: wrong-size-frame discard (Ghostty pattern). If the
        // surface's configured dimensions don't match the live client
        // rect, drop this frame and schedule a repaint so the next
        // iteration renders at the new size. Eliminates the "smear
        // during fast drag" artifact that occurs when WM_SIZE arrives
        // mid-frame.
        //
        // We only get here when no `pending_configure` is in flight
        // (the early return above handles that case), so a mismatch
        // here means an out-of-band rect change (e.g., DWM-induced
        // EXITSIZEMOVE adjustment) that didn't go through our resize
        // event path. InvalidateRect kicks `apply_pending_resize` to
        // start a fresh bg configure on the next paint.
        #[cfg(windows)]
        {
            use ::window::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(wh) = webgpu.handle.window_handle() {
                if let RawWindowHandle::Win32(h) = wh.as_raw() {
                    let hwnd = h.hwnd.get() as winapi::shared::windef::HWND;
                    let (cw, ch) = ::window::os::windows::current_client_size(hwnd);
                    let cfg = webgpu.config.borrow();
                    if cw > 0 && ch > 0 && (cw != cfg.width || ch != cfg.height) {
                        log::debug!(
                            "[render] dropping wrong-size frame (webgpu): \
                             surface={}x{}, client={}x{}, self.dims={}x{}",
                            cfg.width,
                            cfg.height,
                            cw,
                            ch,
                            self.dimensions.pixel_width,
                            self.dimensions.pixel_height,
                        );
                        drop(cfg);
                        unsafe {
                            winapi::um::winuser::InvalidateRect(hwnd, std::ptr::null(), 0);
                        }
                        return Ok(());
                    }
                }
            }
        }
        // --- end weezterm remote features ---

        // --- weezterm remote features ---
        let cfg_w;
        let cfg_h;
        {
            let cfg = webgpu.config.borrow();
            cfg_w = cfg.width;
            cfg_h = cfg.height;
        }
        // --- end weezterm remote features ---
        let surface_borrow = webgpu.surface.borrow();
        let surface_ref = match surface_borrow.as_ref() {
            Some(s) => s,
            None => {
                // No active surface — bg configure must be in
                // flight (or just failed). Skip this frame; the
                // bg thread's InvalidateRect (or the next
                // resize/timer tick) will trigger another paint.
                log::debug!("[render] no surface (bg configure in flight); skipping frame");
                return Ok(());
            }
        };
        let output = surface_ref.get_current_texture()?;
        drop(surface_borrow);
        // --- weezterm remote features ---
        log::debug!(
            "[render] call_draw_webgpu cfg={}x{} self.dims={}x{} got_texture={}x{} suboptimal={}",
            cfg_w,
            cfg_h,
            self.dimensions.pixel_width,
            self.dimensions.pixel_height,
            output.texture.width(),
            output.texture.height(),
            output.suboptimal,
        );
        // --- end weezterm remote features ---
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = webgpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        let tex = render_state.glyph_cache.borrow().atlas.texture();
        let tex = tex.downcast_ref::<WebGpuTexture>().unwrap();
        let texture_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_linear_bind_group =
            webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&webgpu.texture_linear_sampler),
                    },
                ],
                label: Some("linear bind group"),
            });

        let texture_nearest_bind_group =
            webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&webgpu.texture_nearest_sampler),
                    },
                ],
                label: Some("nearest bind group"),
            });

        let mut cleared = false;
        let foreground_text_hsb = self.config.foreground_text_hsb;
        let foreground_text_hsb = [
            foreground_text_hsb.hue,
            foreground_text_hsb.saturation,
            foreground_text_hsb.brightness,
        ];

        let milliseconds = self.created.elapsed().as_millis() as u32;
        let projection = euclid::Transform3D::<f32, f32, f32>::ortho(
            -(self.dimensions.pixel_width as f32) / 2.0,
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
            -(self.dimensions.pixel_height as f32) / 2.0,
            -1.0,
            1.0,
        )
        .to_arrays_transposed();

        for layer in render_state.layers.borrow().iter() {
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let (vertex_count, index_count) = vb.vertex_index_count();
                let vertex_buffer;
                let uniforms;
                if vertex_count > 0 {
                    let mut vertices = vb.current_vb_mut();
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if cleared {
                                    wgpu::LoadOp::Load
                                } else {
                                    wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.,
                                        g: 0.,
                                        b: 0.,
                                        a: 0.,
                                    })
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    cleared = true;

                    uniforms = webgpu.create_uniform(ShaderUniform {
                        foreground_text_hsb,
                        milliseconds,
                        projection,
                    });

                    render_pass.set_pipeline(&webgpu.render_pipeline);
                    render_pass.set_bind_group(0, &uniforms, &[]);
                    render_pass.set_bind_group(1, &texture_linear_bind_group, &[]);
                    render_pass.set_bind_group(2, &texture_nearest_bind_group, &[]);
                    vertex_buffer = vertices.webgpu_mut().recreate();
                    vertex_buffer.unmap();
                    render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(vb.indices.webgpu().slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..index_count as _, 0, 0..1);
                }

                vb.next_index();
            }
        }

        // submit will accept anything that implements IntoIter
        webgpu.queue.submit(std::iter::once(encoder.finish()));
        // --- weezterm remote features ---
        let cfg_w_log;
        let cfg_h_log;
        {
            let cfg = webgpu.config.borrow();
            cfg_w_log = cfg.width;
            cfg_h_log = cfg.height;
        }
        log::debug!(
            "[render] presenting frame tex={}x{} cfg={}x{} cleared={}",
            output.texture.width(),
            output.texture.height(),
            cfg_w_log,
            cfg_h_log,
            cleared
        );
        // --- end weezterm remote features ---
        output.present();
        // --- weezterm remote features ---
        log::debug!("[render] present returned");
        // --- end weezterm remote features ---

        Ok(())
    }

    fn call_draw_glium(&mut self, frame: &mut glium::Frame) -> anyhow::Result<()> {
        use window::glium::texture::SrgbTexture2d;

        let gl_state = self.render_state.as_ref().unwrap();
        let tex = gl_state.glyph_cache.borrow().atlas.texture();
        let tex = tex.downcast_ref::<SrgbTexture2d>().unwrap();

        // --- weezterm remote features ---
        // The OpenGL pixel format requests 8 alpha bits and the window
        // calls `DwmEnableBlurBehindWindow`, which together cause DWM
        // to honour the framebuffer alpha channel for compositing.
        // If we cleared with alpha=0, every pixel not subsequently
        // written by a draw call (e.g. window-resize that races the
        // terminal-size update, or the brief one-frame window between
        // `assign_overlay` and the overlay's bg thread writing its
        // first prompt) would be transparent — exposing the desktop or
        // any window behind us. On the wgpu / DXGI HWND-swapchain path
        // this never showed up because HWND swapchains are forced opaque
        // by DXGI; on the OpenGL+Mesa path (now the default on RDP) it
        // is very visible.
        //
        // Use alpha=1.0 when the user wants an opaque window so the
        // cleared framebuffer is opaque-black even where the renderer
        // doesn't paint over it. Translucent users (window_background_
        // opacity < 1.0 or a window_background_image) keep alpha=0 so
        // their compositing behaves as before.
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;
        let clear_alpha = if window_is_transparent { 0. } else { 1. };
        frame.clear_color(0., 0., 0., clear_alpha);
        // --- end weezterm remote features ---

        let projection = euclid::Transform3D::<f32, f32, f32>::ortho(
            -(self.dimensions.pixel_width as f32) / 2.0,
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
            -(self.dimensions.pixel_height as f32) / 2.0,
            -1.0,
            1.0,
        )
        .to_arrays_transposed();

        let use_subpixel = match self
            .config
            .freetype_render_target
            .unwrap_or(self.config.freetype_load_target)
        {
            FreeTypeLoadTarget::HorizontalLcd | FreeTypeLoadTarget::VerticalLcd => true,
            _ => false,
        };

        let dual_source_blending = glium::DrawParameters {
            blend: glium::Blend {
                color: BlendingFunction::Addition {
                    source: LinearBlendingFactor::SourceOneColor,
                    destination: LinearBlendingFactor::OneMinusSourceOneColor,
                },
                alpha: BlendingFunction::Addition {
                    source: LinearBlendingFactor::SourceOneColor,
                    destination: LinearBlendingFactor::OneMinusSourceOneColor,
                },
                constant_value: (0.0, 0.0, 0.0, 0.0),
            },

            ..Default::default()
        };

        let alpha_blending = glium::DrawParameters {
            blend: glium::Blend {
                color: BlendingFunction::Addition {
                    source: LinearBlendingFactor::SourceAlpha,
                    destination: LinearBlendingFactor::OneMinusSourceAlpha,
                },
                alpha: BlendingFunction::Addition {
                    source: LinearBlendingFactor::One,
                    destination: LinearBlendingFactor::OneMinusSourceAlpha,
                },
                constant_value: (0.0, 0.0, 0.0, 0.0),
            },
            ..Default::default()
        };

        // Clamp and use the nearest texel rather than interpolate.
        // This prevents things like the box cursor outlines from
        // being randomly doubled in width or height
        let atlas_nearest_sampler = Sampler::new(&*tex)
            .wrap_function(SamplerWrapFunction::Clamp)
            .magnify_filter(MagnifySamplerFilter::Nearest)
            .minify_filter(MinifySamplerFilter::Nearest);

        let atlas_linear_sampler = Sampler::new(&*tex)
            .wrap_function(SamplerWrapFunction::Clamp)
            .magnify_filter(MagnifySamplerFilter::Linear)
            .minify_filter(MinifySamplerFilter::Linear);

        let foreground_text_hsb = self.config.foreground_text_hsb;
        let foreground_text_hsb = (
            foreground_text_hsb.hue,
            foreground_text_hsb.saturation,
            foreground_text_hsb.brightness,
        );

        let milliseconds = self.created.elapsed().as_millis() as u32;

        let cursor_blink: ColorEaseUniform = (*self.cursor_blink_state.borrow()).into();
        let blink: ColorEaseUniform = (*self.blink_state.borrow()).into();
        let rapid_blink: ColorEaseUniform = (*self.rapid_blink_state.borrow()).into();

        for layer in gl_state.layers.borrow().iter() {
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let (vertex_count, index_count) = vb.vertex_index_count();
                if vertex_count > 0 {
                    let vertices = vb.current_vb_mut();
                    let subpixel_aa = use_subpixel && idx == 1;

                    let mut uniforms = UniformBuilder::default();

                    uniforms.add("projection", &projection);
                    uniforms.add("atlas_nearest_sampler", &atlas_nearest_sampler);
                    uniforms.add("atlas_linear_sampler", &atlas_linear_sampler);
                    uniforms.add("foreground_text_hsb", &foreground_text_hsb);
                    uniforms.add("subpixel_aa", &subpixel_aa);
                    uniforms.add("milliseconds", &milliseconds);
                    uniforms.add_struct("cursor_blink", &cursor_blink);
                    uniforms.add_struct("blink", &blink);
                    uniforms.add_struct("rapid_blink", &rapid_blink);

                    frame.draw(
                        vertices.glium().slice(0..vertex_count).unwrap(),
                        vb.indices.glium().slice(0..index_count).unwrap(),
                        gl_state.glyph_prog.as_ref().unwrap(),
                        &uniforms,
                        if subpixel_aa {
                            &dual_source_blending
                        } else {
                            &alpha_blending
                        },
                    )?;
                }

                vb.next_index();
            }
        }

        Ok(())
    }
}
