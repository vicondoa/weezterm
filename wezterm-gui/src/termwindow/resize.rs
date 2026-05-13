use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::utilsprites::RenderMetrics;
use ::window::{Dimensions, ResizeIncrement, Window, WindowOps, WindowState};
use config::{ConfigHandle, DimensionContext};
use mux::Mux;
use std::rc::Rc;
use wezterm_font::FontConfiguration;
use wezterm_term::TerminalSize;

#[derive(Debug, Clone, Copy)]
pub struct RowsAndCols {
    pub rows: usize,
    pub cols: usize,
}

#[derive(Debug)]
pub enum ScaleChange {
    Absolute(f64),
    Relative(f64),
}

impl super::TermWindow {
    pub fn resize(
        &mut self,
        dimensions: Dimensions,
        window_state: WindowState,
        window: &Window,
        live_resizing: bool,
    ) {
        // --- weezterm remote features ---
        let _t = std::time::Instant::now();
        log::debug!(
            "[resize] event live={} current_cells={:?} current_dims={:?} new_dims={:?} state={:?}",
            live_resizing,
            self.current_cell_dimensions(),
            self.dimensions,
            dimensions,
            window_state,
        );
        // --- end weezterm remote features ---
        if dimensions.pixel_width == 0 || dimensions.pixel_height == 0 {
            // on windows, this can happen when minimizing the window.
            // NOP!
            log::trace!("new dimensions are zero: NOP!");
            return;
        }
        // --- weezterm remote features ---
        // Even if the dims haven't changed, we MUST propagate the
        // `live_resizing` transition to the GPU surface so it can
        // drop its debounce and apply the deferred surface.configure
        // for the final size. The dispatch fired by
        // `wm_enter_exit_size_move` on WM_EXITSIZEMOVE often arrives
        // with dims == self.dimensions (the last drag step already
        // updated us), so we cannot rely on the "real change" path
        // below to do this.
        let was_live_resize = if let Some(webgpu) = self.backend.webgpu() {
            let prev = webgpu.is_live_resize_active();
            webgpu.set_live_resize_active(live_resizing);
            prev
        } else {
            false
        };
        // --- end weezterm remote features ---
        if self.dimensions == dimensions && self.window_state == window_state {
            // It didn't really change
            log::trace!("dimensions didn't change NOP!");
            // --- weezterm remote features ---
            // BUT: if we just transitioned live -> idle, the swap
            // chain is still at the OLD pre-drag size and DWM is
            // displaying it stretched to fill the new client rect.
            // We MUST kick a fresh paint so apply_pending_resize
            // runs and the new frame is rendered at the right size.
            // We also bump quad_generation to invalidate any cached
            // scene that might have been built for the old viewport.
            if was_live_resize && !live_resizing {
                log::debug!(
                    "[resize] live->idle transition with unchanged dims; \
                     forcing repaint to apply deferred surface.configure"
                );
                self.quad_generation += 1;
                self.invalidate_fancy_tab_bar();
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
            }
            // --- end weezterm remote features ---
            return;
        }
        let last_state = self.window_state;
        self.window_state = window_state;
        self.quad_generation += 1;
        if last_state != self.window_state {
            self.load_os_parameters();
        }

        // --- weezterm remote features ---
        // Phase 4d (perf): reorder to ensure the terminal grid
        // (rows/cols) is recomputed for the new dims BEFORE we
        // touch the GPU surface. webgpu.resize() now only stores
        // the pending dims (surface.configure is deferred to the
        // render path), so this ordering guarantees that when the
        // next paint runs, BOTH the swapchain AND the grid match
        // the new client rect — eliminating the
        // "snap to original (small) size in big window"
        // intermediate frame the user sees during drag-to-grow.
        let _t_apply = std::time::Instant::now();
        if self.dimensions.dpi == dimensions.dpi {
            self.apply_dimensions(&dimensions, None, window);
        } else {
            self.scaling_changed(dimensions, self.fonts.get_font_scale(), window);
        }
        log::debug!(
            "[resize] apply_dimensions/scaling_changed took {:?}",
            _t_apply.elapsed()
        );
        // --- end weezterm remote features ---

        if let Some(webgpu) = self.backend.webgpu() {
            // --- weezterm remote features ---
            // Tell the surface whether we're in a live drag so it
            // can decide whether to debounce its `surface.configure`
            // (the slow DXGI ResizeBuffers call). During an active
            // drag we keep the OS-stretched stale frame on screen
            // and avoid blocking the UI thread.
            webgpu.set_live_resize_active(live_resizing);
            let _t_gpu = std::time::Instant::now();
            // --- end weezterm remote features ---
            webgpu.resize(dimensions);
            // --- weezterm remote features ---
            log::debug!("[resize] webgpu.resize took {:?}", _t_gpu.elapsed());
            // --- end weezterm remote features ---
        }
        // --- weezterm remote features ---
        // Phase 4 Mode C: keep the WARP swap chain in sync with the
        // window client rect.
        #[cfg(windows)]
        if let Some(state) = self.backend.software_rdp() {
            let _t_swrast = std::time::Instant::now();
            let mut s = state.borrow_mut();
            if let Err(err) = s.resize(
                dimensions.pixel_width as u32,
                dimensions.pixel_height as u32,
            ) {
                log::error!("[render] SoftwareRdp resize failed: {err:#}");
            }
            drop(s);
            if let Some(cpu) = self.cpu_renderer.as_mut() {
                cpu.invalidate_all();
            }
            log::debug!(
                "[resize] software_rdp.resize took {:?}",
                _t_swrast.elapsed()
            );
        }
        // --- end weezterm remote features ---
        if let Some(modal) = self.get_modal() {
            modal.reconfigure(self);
        }
        // --- weezterm remote features ---
        let _t_emit = std::time::Instant::now();
        // --- end weezterm remote features ---
        self.emit_window_event("window-resized", None);
        // --- weezterm remote features ---
        log::debug!("[resize] emit_window_event took {:?}", _t_emit.elapsed());
        log::debug!("[resize] event completed in {:?}", _t.elapsed());
        // --- end weezterm remote features ---
    }

    pub fn apply_pending_scale_changes(&mut self) {
        while self.resizes_pending == 0 {
            match self.pending_scale_changes.pop_front() {
                Some(ScaleChange::Relative(change)) => {
                    if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
                        self.adjust_font_scale(self.fonts.get_font_scale() * change, &window);
                    }
                }
                Some(ScaleChange::Absolute(change)) => {
                    if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
                        self.adjust_font_scale(change, &window);
                    }
                }
                None => break,
            }
        }
    }

    pub fn apply_scale_change(&mut self, dimensions: &Dimensions, font_scale: f64) {
        let _t = std::time::Instant::now();
        log::debug!(
            "apply_scale_change: dims={:?} font_scale={} dpi={}",
            dimensions,
            font_scale,
            dimensions.dpi
        );
        let config = &self.config;
        let font_size = config.font_size * font_scale;
        let theoretical_height = font_size * dimensions.dpi as f64 / 72.0;

        if theoretical_height < 2.0 {
            log::warn!(
                "refusing to go to an unreasonably small font scale {:?}
                       font_scale={} would yield font_height {}",
                dimensions,
                font_scale,
                theoretical_height
            );
            return;
        }

        let (prior_font, prior_dpi) = self.fonts.change_scaling(font_scale, dimensions.dpi);
        match RenderMetrics::new(&self.fonts) {
            Ok(metrics) => {
                self.render_metrics = metrics;
            }
            Err(err) => {
                log::error!(
                    "{:#} while attempting to scale font to {} with {:?}",
                    err,
                    font_scale,
                    dimensions
                );
                // Restore prior scaling factors
                self.fonts.change_scaling(prior_font, prior_dpi);
            }
        }

        if let Err(err) = self.recreate_texture_atlas(None) {
            log::error!("recreate_texture_atlas: {:#}", err);
        }
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
        log::debug!("apply_scale_change completed in {:?}", _t.elapsed());
    }

    pub fn apply_dimensions(
        &mut self,
        dimensions: &Dimensions,
        mut scale_changed_cells: Option<RowsAndCols>,
        window: &Window,
    ) {
        let _t = std::time::Instant::now();
        log::trace!(
            "apply_dimensions {:?} scale_changed_cells {:?}. window_state {:?}",
            dimensions,
            scale_changed_cells,
            self.window_state
        );
        let saved_dims = self.dimensions;
        self.dimensions = *dimensions;
        self.quad_generation += 1;

        if scale_changed_cells.is_some() && !self.window_state.can_resize() {
            log::warn!(
                "cannot resize window to match {:?} because window_state is {:?}",
                scale_changed_cells,
                self.window_state
            );
            scale_changed_cells.take();
            // --- weezterm remote features ---
            // When window can't resize (maximized/fullscreen), the dimensions
            // parameter may be synthetic (from set_window_size). Restore to
            // the actual window dimensions to prevent corruption that would
            // cause subsequent resize events to NOP.
            self.dimensions = saved_dims;
            // --- end weezterm remote features ---
        }

        // Technically speaking, we should compute the rows and cols
        // from the new dimensions and apply those to the tabs, and
        // then for the scaling changed case, try to re-apply the
        // original rows and cols, but if we do that we end up
        // double resizing the tabs, so we speculatively apply the
        // final size, which in that case should result in a NOP
        // change to the tab size.

        let config = &self.config;

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height().unwrap_or(0.)
        } else {
            0.
        };

        let border = self.get_os_border();

        let (size, dims, ri_calc) = if let Some(cell_dims) = scale_changed_cells {
            // Scaling preserves existing terminal dimensions, yielding a new
            // overall set of window dimensions
            let size = TerminalSize {
                rows: cell_dims.rows,
                cols: cell_dims.cols,
                pixel_height: cell_dims.rows * self.render_metrics.cell_size.height as usize,
                pixel_width: cell_dims.cols * self.render_metrics.cell_size.width as usize,
                dpi: dimensions.dpi as u32,
            };

            let rows = size.rows;
            let cols = size.cols;

            let h_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: size.pixel_width as f32,
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
            let padding_top = config.window_padding.top.evaluate_as_pixels(v_context) as usize;
            let padding_bottom =
                config.window_padding.bottom.evaluate_as_pixels(v_context) as usize;
            let padding_right = effective_right_padding(&config, h_context);

            let pixel_height = (rows * self.render_metrics.cell_size.height as usize)
                + (padding_top + padding_bottom)
                + (border.top + border.bottom).get() as usize
                + tab_bar_height as usize;

            let pixel_width = (cols * self.render_metrics.cell_size.width as usize)
                + (padding_left + padding_right)
                + (border.left + border.right).get() as usize;

            let dims = Dimensions {
                pixel_width: pixel_width as usize,
                pixel_height: pixel_height as usize,
                dpi: dimensions.dpi,
            };

            let ri_calc = ResizeIncrementCalculator {
                x: self.render_metrics.cell_size.width as u16,
                y: self.render_metrics.cell_size.height as u16,
                padding_left: padding_left,
                padding_top: padding_top,
                padding_right: padding_right,
                padding_bottom: padding_bottom,
                border: border,
                tab_bar_height: tab_bar_height as usize,
            };

            (size, dims, ri_calc)
        } else {
            // Resize of the window dimensions may result in changed terminal dimensions

            let h_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_width as f32,
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
            let padding_top = config.window_padding.top.evaluate_as_pixels(v_context) as usize;
            let padding_bottom =
                config.window_padding.bottom.evaluate_as_pixels(v_context) as usize;
            let padding_right = effective_right_padding(&config, h_context);

            let avail_width = dimensions.pixel_width.saturating_sub(
                (padding_left + padding_right) as usize
                    + (border.left + border.right).get() as usize,
            );
            let avail_height = dimensions
                .pixel_height
                .saturating_sub(
                    (padding_top + padding_bottom) as usize
                        + (border.top + border.bottom).get() as usize,
                )
                .saturating_sub(tab_bar_height as usize);

            let rows = avail_height / self.render_metrics.cell_size.height as usize;
            let cols = avail_width / self.render_metrics.cell_size.width as usize;

            let size = TerminalSize {
                rows,
                cols,
                // Take care to use the exact pixel dimensions of the cells, rather
                // than the available space, so that apps that are sensitive to
                // the pixels-per-cell have consistent values at a given font size.
                // https://github.com/wezterm/wezterm/issues/535
                pixel_height: rows * self.render_metrics.cell_size.height as usize,
                pixel_width: cols * self.render_metrics.cell_size.width as usize,
                dpi: dimensions.dpi as u32,
            };

            let ri_calc = ResizeIncrementCalculator {
                x: self.render_metrics.cell_size.width as u16,
                y: self.render_metrics.cell_size.height as u16,
                padding_left: padding_left,
                padding_top: padding_top,
                padding_right: padding_right,
                padding_bottom: padding_bottom,
                border: border,
                tab_bar_height: tab_bar_height as usize,
            };

            (size, *dimensions, ri_calc)
        };

        // --- weezterm remote features ---
        log::debug!(
            "[resize] apply_dimensions computed size={:?} dims={:?}",
            size,
            dims
        );
        let _t_compute = std::time::Instant::now();
        // --- end weezterm remote features ---

        self.terminal_size = size;

        let mux = Mux::get();
        if let Some(window) = mux.get_window(self.mux_window_id) {
            // --- weezterm remote features ---
            let tab_count = window.len();
            // --- end weezterm remote features ---
            for tab in window.iter() {
                // --- weezterm remote features ---
                log::debug!(
                    "[resize] tab.resize tab={} size={:?} (window has {} tabs)",
                    tab.tab_id(),
                    size,
                    tab_count,
                );
                let _t_tab = std::time::Instant::now();
                // --- end weezterm remote features ---
                tab.resize(size);
                // --- weezterm remote features ---
                log::debug!(
                    "[resize] tab.resize tab={} took {:?}",
                    tab.tab_id(),
                    _t_tab.elapsed()
                );
                // --- end weezterm remote features ---
            }
        // --- weezterm remote features ---
        } else {
            log::warn!(
                "apply_dimensions: mux.get_window({}) returned None, tabs NOT resized!",
                self.mux_window_id,
            );
            // --- end weezterm remote features ---
        };
        // --- weezterm remote features ---
        let _t_overlays = std::time::Instant::now();
        // --- end weezterm remote features ---
        self.resize_overlays();
        // --- weezterm remote features ---
        log::debug!("[resize] resize_overlays took {:?}", _t_overlays.elapsed());
        let _t_inv = std::time::Instant::now();
        // --- end weezterm remote features ---
        self.invalidate_fancy_tab_bar();
        // --- weezterm remote features ---
        log::debug!(
            "[resize] invalidate_fancy_tab_bar took {:?}",
            _t_inv.elapsed()
        );
        let _t_title = std::time::Instant::now();
        // --- end weezterm remote features ---
        self.update_title();
        // --- weezterm remote features ---
        log::debug!("[resize] update_title took {:?}", _t_title.elapsed());
        log::debug!(
            "[resize] apply_dimensions tail took {:?}",
            _t_compute.elapsed()
        );
        // --- end weezterm remote features ---

        window.set_resize_increments(if self.config.use_resize_increments {
            ri_calc.into()
        } else {
            ResizeIncrement::disabled()
        });

        // Queue up a speculative resize in order to preserve the number of rows+cols
        if let Some(cell_dims) = scale_changed_cells {
            // If we don't think the dimensions have changed, don't request
            // the window to change.  This seems to help on Wayland where
            // we won't know what size the compositor thinks we should have
            // when we're first opened, until after it sends us a configure event.
            // If we send this too early, it will trump that configure event
            // and we'll end up with weirdness where our window renders in the
            // middle of a larger region that the compositor thinks we live in.
            // Wayland is weird!
            if saved_dims != dims {
                log::trace!(
                    "scale changed so resize from {:?} to {:?} {:?} (event called with {:?})",
                    saved_dims,
                    dims,
                    cell_dims,
                    dimensions
                );
                // Stash this size pre-emptively. Without this, on Windows,
                // when the font scaling is changed we can end up not seeing
                // these dimensions and the scaling_changed logic ends up
                // comparing two dimensions that have the same DPI and recomputing
                // an adjusted terminal size.
                // eg: rather than a simple old-dpi -> new dpi transition, we'd
                // see old-dpi -> new dpi, call set_inner_size, then see a
                // new-dpi -> new-dpi adjustment with a slightly different
                // pixel geometry which is considered to be a user-driven resize.
                // Stashing the dimensions here avoids that misconception.
                self.dimensions = dims;
                self.set_inner_size(window, dims.pixel_width, dims.pixel_height);
            }
        }
    }

    pub fn current_cell_dimensions(&self) -> RowsAndCols {
        RowsAndCols {
            rows: self.terminal_size.rows as usize,
            cols: self.terminal_size.cols as usize,
        }
    }

    #[allow(clippy::float_cmp)]
    pub fn scaling_changed(&mut self, dimensions: Dimensions, font_scale: f64, window: &Window) {
        let _t = std::time::Instant::now();
        log::debug!(
            "scaling_changed: dims={:?} font_scale={} dpi={}",
            dimensions,
            font_scale,
            dimensions.dpi
        );
        fn dpi_adjusted(n: usize, dpi: usize) -> f32 {
            n as f32 / dpi as f32
        }

        /// On Windows, scaling changes may adjust the pixel geometry by a few pixels,
        /// so this function checks if we're in a close-enough ballpark.
        fn close_enough(a: f32, b: f32) -> bool {
            let diff = (a - b).abs();
            diff < 10.
        }

        // Distinguish between eg: dpi being detected as double the initial dpi (where
        // the pixel dimensions don't change), and the dpi change being detected, but
        // where the window manager also decides to tile/resize the window.
        // In the latter case, we don't want to preserve the terminal rows/cols.
        let simple_dpi_change = dimensions.dpi != self.dimensions.dpi
            && ((close_enough(
                dpi_adjusted(dimensions.pixel_height, dimensions.dpi),
                dpi_adjusted(self.dimensions.pixel_height, self.dimensions.dpi),
            ) && close_enough(
                dpi_adjusted(dimensions.pixel_width, dimensions.dpi),
                dpi_adjusted(self.dimensions.pixel_width, self.dimensions.dpi),
            )) || (close_enough(
                dimensions.pixel_width as f32,
                self.dimensions.pixel_width as f32,
            ) && close_enough(
                dimensions.pixel_height as f32,
                self.dimensions.pixel_height as f32,
            )));

        if simple_dpi_change && cfg!(target_os = "macos") {
            // Spooky action at a distance: on macOS, NSWindow::isZoomed can falsely
            // return YES in situations such as the current screen changing.
            // That causes window_state to believe that we are MAXIMIZED.
            // We cannot easily detect that in the window layer, but at this
            // layer, if we realize that the dpi was the only thing that changed
            // then remove the MAXIMIZED state so that the can_resize check
            // in adjust_font_scale will not block us from adapting to the new
            // DPI. This is gross and it would be better handled at the macOS
            // layer.
            // <https://github.com/wezterm/wezterm/issues/3503>
            self.window_state -= WindowState::MAXIMIZED;
        }

        let dpi_changed = dimensions.dpi != self.dimensions.dpi;
        let font_scale_changed = font_scale != self.fonts.get_font_scale();
        let scale_changed = dpi_changed || font_scale_changed;

        log::trace!(
            "dpi_changed={}, font_scale_changed={} scale_changed={} simple_dpi_change={}",
            dpi_changed,
            font_scale_changed,
            scale_changed,
            simple_dpi_change
        );

        let cell_dims = self.current_cell_dimensions();

        if scale_changed {
            self.apply_scale_change(&dimensions, font_scale);
        }

        let scale_changed_cells = if font_scale_changed || simple_dpi_change {
            Some(cell_dims)
        } else {
            None
        };

        log::trace!(
            "scaling_changed, follow with applying dimensions. scale_changed_cells={:?}",
            scale_changed_cells
        );
        self.apply_dimensions(&dimensions, scale_changed_cells, window);
        log::debug!("scaling_changed completed in {:?}", _t.elapsed());
    }

    /// Used for applying font size changes only; this takes into account
    /// the `adjust_window_size_when_changing_font_size` configuration and
    /// revises the scaling/resize change accordingly
    pub fn adjust_font_scale(&mut self, font_scale: f64, window: &Window) {
        let adjust_window_size_when_changing_font_size =
            match self.config.adjust_window_size_when_changing_font_size {
                Some(value) => value,
                None => {
                    let is_tiling = self
                        .config
                        .tiling_desktop_environments
                        .iter()
                        .any(|item| item.as_str() == self.connection_name.as_str());
                    !is_tiling
                }
            };

        if self.window_state.can_resize() && adjust_window_size_when_changing_font_size {
            self.scaling_changed(self.dimensions, font_scale, window);
        } else {
            let dimensions = self.dimensions;
            // Compute new font metrics
            self.apply_scale_change(&dimensions, font_scale);
            // Now revise the pty size to fit the window
            self.apply_dimensions(&dimensions, None, window);
        }
    }

    pub fn decrease_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Relative(1.0 / 1.1));
        self.apply_pending_scale_changes();
    }

    pub fn increase_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Relative(1.1));
        self.apply_pending_scale_changes();
    }

    pub fn reset_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Absolute(1.0));
        self.apply_pending_scale_changes();
    }

    pub fn set_window_size(&mut self, size: TerminalSize, window: &Window) -> anyhow::Result<()> {
        let config = &self.config;
        let fontconfig = Rc::new(FontConfiguration::new(
            Some(config.clone()),
            self.dimensions.dpi,
        )?);
        let render_metrics = RenderMetrics::new(&fontconfig)?;

        let terminal_size = TerminalSize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: (render_metrics.cell_size.width as usize * size.cols),
            pixel_height: (render_metrics.cell_size.height as usize * size.rows),
            dpi: size.dpi,
        };

        let show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        let tab_bar_height = if show_tab_bar {
            self.tab_bar_pixel_height()? as usize
        } else {
            0
        };

        let h_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_width as f32,
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        let v_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_height as f32,
            pixel_cell: render_metrics.cell_size.height as f32,
        };
        let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
        let padding_top = config.window_padding.top.evaluate_as_pixels(v_context) as usize;
        let padding_bottom = config.window_padding.bottom.evaluate_as_pixels(v_context) as usize;

        let dimensions = Dimensions {
            pixel_width: ((terminal_size.cols as usize * render_metrics.cell_size.width as usize)
                + padding_left
                + effective_right_padding(&config, h_context)),
            pixel_height: ((terminal_size.rows as usize * render_metrics.cell_size.height as usize)
                + padding_top
                + padding_bottom) as usize
                + tab_bar_height,
            dpi: self.dimensions.dpi,
        };

        self.apply_scale_change(&dimensions, 1.0);
        self.apply_dimensions(
            &dimensions,
            Some(RowsAndCols {
                rows: size.rows as usize,
                cols: size.cols as usize,
            }),
            window,
        );
        Ok(())
    }

    pub fn reset_font_and_window_size(&mut self, window: &Window) -> anyhow::Result<()> {
        let size = self.config.initial_size(
            self.dimensions.dpi as u32,
            Some(crate::cell_pixel_dims(
                &self.config,
                self.dimensions.dpi as f64,
            )?),
        );
        self.set_window_size(size, window)
    }

    pub fn effective_right_padding(&self, config: &ConfigHandle) -> usize {
        effective_right_padding(
            config,
            DimensionContext {
                pixel_cell: self.render_metrics.cell_size.width as f32,
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_width as f32,
            },
        )
    }
}

/// Computes the effective padding for the RHS.
/// This is needed because the default is 0, but if the user has
/// enabled the scroll bar then they will expect it to have a reasonable
/// size unless they've specified differently.
pub fn effective_right_padding(config: &ConfigHandle, context: DimensionContext) -> usize {
    if config.enable_scroll_bar && config.window_padding.right.is_zero() {
        context.pixel_cell as usize
    } else {
        config.window_padding.right.evaluate_as_pixels(context) as usize
    }
}
