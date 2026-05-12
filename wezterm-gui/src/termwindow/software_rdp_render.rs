//! Phase 4c: CPU rasteriser feeding the SoftwareRdp WARP swap chain.
//!
//! Walks the active pane's visible rows, fills cell backgrounds, alpha-blends
//! freetype-rasterised glyphs, draws the cursor, and emits a list of dirty
//! cell-row rectangles to feed back into `IDXGISwapChain1::Present1`.
//!
//! This is intentionally a **minimal** renderer:
//!   * No ligatures-by-cluster - we shape each cell's grapheme on its own and
//!     accept the one-glyph-per-cell limitation. Adequate for terminal text;
//!     ligature support is a future refinement.
//!   * No image cells, no sixel, no kitty graphics.
//!   * No background image / blur.
//!   * No bidi reordering beyond what `LoadedFont::shape` does internally.
//!   * Underline / strike-through are drawn as solid 1px rectangles.
//!
//! The point of Mode C is *correct, low-bandwidth* rendering on RDP boxes
//! where we have neither a real GPU nor a usable WGL surface; we trade
//! visual fidelity for liveness. Anything richer is the GPU paths' job.

use super::software_rdp::{DirtyRect, SoftwareRdpState};
use crate::utilsprites::RenderMetrics;
use anyhow::Result;
use mux::pane::Pane;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use termwiz::color::SrgbaTuple;
use termwiz::surface::SequenceNo;
use wezterm_bidi::Direction;
use wezterm_font::shaper::PresentationWidth;
use wezterm_font::FontConfiguration;
use wezterm_term::color::ColorPalette;
use wezterm_term::StableRowIndex;

/// Maximum glyphs we cache before nuking the cache. Each entry is a
/// per-grapheme rasterised bitmap; 4096 covers roughly an entire CJK
/// session without growing unbounded.
const GLYPH_CACHE_CAP: usize = 4096;

/// One pre-rasterised, pre-multiplied-RGBA bitmap for a single glyph.
struct CachedGlyph {
    /// Pre-multiplied RGBA bytes (rows tightly packed, `width * 4`).
    rgba: Vec<u8>,
    width: usize,
    height: usize,
    bearing_x: f64,
    bearing_y: f64,
    /// True if the glyph already carries colour (emoji); we skip
    /// fg-tint multiplication in that case.
    has_color: bool,
}

/// CPU renderer state attached to a `SoftwareRdpState`.
pub struct CpuRenderer {
    /// Glyph cache keyed by the grapheme cluster string. The same
    /// codepoint always produces the same bitmap regardless of fg colour
    /// because we tint at blit time.
    cache: HashMap<String, Rc<CachedGlyph>>,
    /// Per-row sequence numbers from the previous render, so we can mark
    /// only changed rows dirty and skip Present1 rectangles for the rest.
    last_row_seqno: HashMap<StableRowIndex, SequenceNo>,
    /// True until the first successful render; forces a full present.
    fresh: bool,
}

impl CpuRenderer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::with_capacity(256),
            last_row_seqno: HashMap::with_capacity(256),
            fresh: true,
        }
    }

    /// Force the next render to repaint every row (e.g. after a resize
    /// or palette change).
    pub fn invalidate_all(&mut self) {
        self.last_row_seqno.clear();
        self.fresh = true;
    }

    /// Render the active pane into the swap chain's scratch buffer and
    /// queue dirty rectangles for the next `present()`.
    pub fn render(
        &mut self,
        state: &mut SoftwareRdpState,
        pane: &Arc<dyn Pane>,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        palette: &ColorPalette,
    ) -> Result<()> {
        if self.cache.len() > GLYPH_CACHE_CAP {
            self.cache.clear();
        }

        // Take ownership of the dirty-rect list for this frame. Earlier
        // mutations (e.g. swap-chain resize calling mark_all_dirty
        // internally) would otherwise leave a full-canvas rect that
        // overlaps with our bounding-box rect, which Present1 rejects
        // with DXGI_ERROR_INVALID_CALL (0x887a0001).
        state.clear_dirty();

        let dims = pane.get_dimensions();
        let cell_w = metrics.cell_size.width.max(1) as usize;
        let cell_h = metrics.cell_size.height.max(1) as usize;
        let canvas_w = state.width() as usize;
        let canvas_h = state.height() as usize;

        // Always paint the entire backdrop with the palette background.
        // This guarantees that areas outside the terminal grid (right and
        // bottom partial-cell padding) get a sensible colour.
        let bg_default = palette.background.as_rgba_u8();
        fill_solid(state, bg_default);

        let font = fonts.default_font()?;

        // Decide which rows are visible (skip the tab bar - p4c does not
        // render it; the tab bar in Mode C lands in a follow-up).
        let viewport_rows = dims.viewport_rows;
        let physical_top = dims.physical_top;

        // Pull the viewport in one shot. `get_lines` returns
        // (first_returned_row, lines).
        let row_range = physical_top..physical_top + viewport_rows as StableRowIndex;
        let (first_row, lines) = pane.get_lines(row_range);

        // Track which rows changed since the previous render so we can
        // mark them dirty individually.
        let mut new_seqnos: HashMap<StableRowIndex, SequenceNo> =
            HashMap::with_capacity(lines.len());

        // Force-full repaint on first render or after invalidate_all().
        let force_full =
            self.fresh || canvas_w == 0 || canvas_h == 0 || self.last_row_seqno.is_empty();

        // Bounding box of all changed pixels this frame. We emit a
        // single dirty rectangle to Present1 because flip-model swap
        // chains forbid overlapping rectangles, and computing a perfect
        // non-overlapping decomposition (cells + cursor + selection) is
        // not worth the complexity for an RDP fallback. A single
        // bounding rect still beats the full-canvas case when only a
        // few rows change.
        let mut dirty_y_min: Option<usize> = None;
        let mut dirty_y_max: Option<usize> = None;

        for (i, line) in lines.iter().enumerate() {
            let row = first_row + i as StableRowIndex;
            let seqno = line.current_seqno();
            new_seqnos.insert(row, seqno);

            let prev = self.last_row_seqno.get(&row).copied();
            let changed = force_full || prev != Some(seqno);
            if !changed {
                continue;
            }

            // Pixel-y of this row.
            let y_top = i * cell_h;
            if y_top >= canvas_h {
                break;
            }
            let row_h = cell_h.min(canvas_h - y_top);
            // Clear the row stripe to default-bg first; per-cell bg fills
            // overdraw it where attributes differ.
            fill_row_stripe(state, y_top, row_h, bg_default);

            // Walk visible cells.
            for cell in line.visible_cells() {
                let col = cell.cell_index();
                let width_cells = cell.width().max(1);
                let x_left = col * cell_w;
                if x_left >= canvas_w {
                    break;
                }
                let cell_pixel_w = (cell_w * width_cells).min(canvas_w - x_left);

                let attrs = cell.attrs();
                let reverse = attrs.reverse();
                let fg_attr = attrs.foreground();
                let bg_attr = attrs.background();
                let mut fg = palette.resolve_fg(fg_attr);
                let mut bg = palette.resolve_bg(bg_attr);
                if reverse {
                    std::mem::swap(&mut fg, &mut bg);
                }

                // Background: only fill if non-default - the row stripe
                // already covers default-bg cells.
                let bg_u8 = bg.as_rgba_u8();
                if bg_u8 != bg_default {
                    fill_rect(state, x_left, y_top, cell_pixel_w, cell_h, bg_u8);
                }

                // Glyph: rasterise/blit only for non-blank cells.
                let s = cell.str();
                if !s.is_empty() && s != " " {
                    self.draw_cell_glyph(
                        state,
                        &font,
                        s,
                        x_left,
                        y_top,
                        cell_pixel_w,
                        cell_h,
                        &fg,
                        metrics,
                    )?;
                }

                // Underline (single only - double/dashed are future work).
                if attrs.underline() != termwiz::cell::Underline::None {
                    let uy = y_top
                        + (metrics.descender_row as usize)
                            .saturating_sub(metrics.underline_height as usize);
                    let uh = (metrics.underline_height as usize).max(1);
                    fill_rect(state, x_left, uy, cell_pixel_w, uh, fg.as_rgba_u8());
                }
                if attrs.strikethrough() {
                    let sy = y_top + (cell_h / 2);
                    let sh = (metrics.underline_height as usize).max(1);
                    fill_rect(state, x_left, sy, cell_pixel_w, sh, fg.as_rgba_u8());
                }
            }

            // Grow the bounding box for this row.
            let y_end = y_top + row_h;
            dirty_y_min = Some(dirty_y_min.map_or(y_top, |v| v.min(y_top)));
            dirty_y_max = Some(dirty_y_max.map_or(y_end, |v| v.max(y_end)));
        }

        // Cursor. Returns its (y, h) so we can extend the bounding box;
        // the cursor pixel rect is contained within the dirty box.
        if let Some((cy, ch)) =
            self.draw_cursor(state, pane, palette, metrics, first_row, lines.len())
        {
            let y_end = cy + ch;
            dirty_y_min = Some(dirty_y_min.map_or(cy, |v| v.min(cy)));
            dirty_y_max = Some(dirty_y_max.map_or(y_end, |v| v.max(y_end)));
        }

        // Emit one bounding-box dirty rect for Present1, or fall through
        // to the present() force-full path if absolutely nothing changed.
        if let (Some(y0), Some(y1)) = (dirty_y_min, dirty_y_max) {
            if y1 > y0 && canvas_w > 0 {
                state.mark_dirty(DirtyRect {
                    x: 0,
                    y: y0 as i32,
                    w: canvas_w as u32,
                    h: (y1 - y0) as u32,
                });
            }
        }

        // Update bookkeeping.
        self.last_row_seqno = new_seqnos;
        self.fresh = false;
        Ok(())
    }

    fn draw_cell_glyph(
        &mut self,
        state: &mut SoftwareRdpState,
        font: &Rc<wezterm_font::LoadedFont>,
        text: &str,
        x_left: usize,
        y_top: usize,
        cell_pixel_w: usize,
        cell_h: usize,
        fg: &SrgbaTuple,
        metrics: &RenderMetrics,
    ) -> Result<()> {
        // Strip variation-selectors that the shaper handles internally.
        let key = text.to_string();
        let cached = if let Some(g) = self.cache.get(&key) {
            g.clone()
        } else {
            let glyph = match Self::rasterise(font, text) {
                Ok(g) => g,
                Err(_) => return Ok(()),
            };
            let rc = Rc::new(glyph);
            self.cache.insert(key, rc.clone());
            rc
        };

        if cached.width == 0 || cached.height == 0 {
            return Ok(());
        }

        // Place the glyph: pen at baseline. Baseline-y inside the cell is
        // `cell_h + descender_row` ish; use simpler:
        //   baseline_y = cell_h + descender_row (descender_row is signed).
        // The freetype rasteriser's bearing_y is the distance from
        // baseline to the top of the bitmap, so the bitmap top in cell
        // coords is `baseline_y - bearing_y`.
        let baseline_y = (cell_h as isize) + metrics.descender_row;
        let top_in_cell = baseline_y - cached.bearing_y as isize;
        let glyph_y = y_top as isize + top_in_cell;
        let glyph_x = x_left as isize + cached.bearing_x as isize;

        blit_alpha(
            state,
            &cached,
            glyph_x,
            glyph_y,
            *fg,
            // Clip to the cell so wide glyphs don't bleed into neighbours
            // (we already widened cell_pixel_w for double-wide cells).
            x_left,
            cell_pixel_w,
        );
        Ok(())
    }

    fn rasterise(font: &Rc<wezterm_font::LoadedFont>, text: &str) -> Result<CachedGlyph> {
        // Use a no-op completion / filter; shape is sync for cached glyphs.
        let info = font.shape(
            text,
            || {},
            |_| {},
            None,
            Direction::LeftToRight,
            None,
            None as Option<&PresentationWidth>,
        )?;
        // Take the first (and usually only) glyph.
        let g = match info.into_iter().next() {
            Some(g) => g,
            None => {
                return Ok(CachedGlyph {
                    rgba: vec![],
                    width: 0,
                    height: 0,
                    bearing_x: 0.0,
                    bearing_y: 0.0,
                    has_color: false,
                });
            }
        };
        let raster = font.rasterize_glyph(g.glyph_pos, g.font_idx)?;
        Ok(CachedGlyph {
            rgba: raster.data,
            width: raster.width,
            height: raster.height,
            bearing_x: raster.bearing_x.get() + g.x_offset.get(),
            bearing_y: raster.bearing_y.get() + g.y_offset.get(),
            has_color: raster.has_color,
        })
    }

    fn draw_cursor(
        &self,
        state: &mut SoftwareRdpState,
        pane: &Arc<dyn Pane>,
        palette: &ColorPalette,
        metrics: &RenderMetrics,
        first_row: StableRowIndex,
        n_rows: usize,
    ) -> Option<(usize, usize)> {
        let cur = pane.get_cursor_position();
        if cur.visibility != termwiz::surface::CursorVisibility::Visible {
            return None;
        }
        if cur.y < first_row || cur.y >= first_row + n_rows as StableRowIndex {
            return None;
        }
        let row = (cur.y - first_row) as usize;
        let cell_w = metrics.cell_size.width.max(1) as usize;
        let cell_h = metrics.cell_size.height.max(1) as usize;
        let canvas_w = state.width() as usize;
        let canvas_h = state.height() as usize;
        let x = cur.x * cell_w;
        let y = row * cell_h;
        if x >= canvas_w || y >= canvas_h {
            return None;
        }
        let w = cell_w.min(canvas_w - x);
        let h = cell_h.min(canvas_h - y);
        let bg = palette.cursor_bg.as_rgba_u8();
        // Block shape only for now; bar/underline shapes land in p4d.
        fill_rect(state, x, y, w, h, bg);
        Some((y, h))
    }

    /// Returns the glyph-cache size, used by tests/diagnostics.
    #[allow(dead_code)]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

/// Fill the entire scratch buffer with one BGRA colour.
fn fill_solid(state: &mut SoftwareRdpState, rgba: (u8, u8, u8, u8)) {
    let (pixels, stride) = state.pixels_mut();
    let h = pixels.len() / stride as usize;
    for y in 0..h {
        let row = &mut pixels[y * stride as usize..(y + 1) * stride as usize];
        for px in row.chunks_exact_mut(4) {
            px[0] = rgba.2; // B
            px[1] = rgba.1; // G
            px[2] = rgba.0; // R
            px[3] = 0xff;
        }
    }
}

/// Fill `h` rows starting at `y_top`.
fn fill_row_stripe(state: &mut SoftwareRdpState, y_top: usize, h: usize, rgba: (u8, u8, u8, u8)) {
    let (pixels, stride) = state.pixels_mut();
    let stride = stride as usize;
    let total_h = pixels.len() / stride;
    let y_end = (y_top + h).min(total_h);
    for y in y_top..y_end {
        let row = &mut pixels[y * stride..(y + 1) * stride];
        for px in row.chunks_exact_mut(4) {
            px[0] = rgba.2;
            px[1] = rgba.1;
            px[2] = rgba.0;
            px[3] = 0xff;
        }
    }
}

fn fill_rect(
    state: &mut SoftwareRdpState,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: (u8, u8, u8, u8),
) {
    let (pixels, stride) = state.pixels_mut();
    let stride = stride as usize;
    let total_h = pixels.len() / stride;
    let y_end = (y + h).min(total_h);
    let bytes_per_row = stride / 4;
    let x_end = (x + w).min(bytes_per_row);
    if x >= x_end || y >= y_end {
        return;
    }
    for yy in y..y_end {
        let row = &mut pixels[yy * stride + x * 4..yy * stride + x_end * 4];
        for px in row.chunks_exact_mut(4) {
            px[0] = rgba.2;
            px[1] = rgba.1;
            px[2] = rgba.0;
            px[3] = 0xff;
        }
    }
}

/// Blit a pre-multiplied RGBA glyph onto the BGRA scratch buffer with
/// `fg` tint (skipped for colour glyphs). `clip_x0` / `clip_w` constrain
/// the destination to the owning cell so wide glyphs do not leak.
fn blit_alpha(
    state: &mut SoftwareRdpState,
    glyph: &CachedGlyph,
    dst_x: isize,
    dst_y: isize,
    fg: SrgbaTuple,
    clip_x0: usize,
    clip_w: usize,
) {
    if glyph.width == 0 || glyph.height == 0 || glyph.rgba.is_empty() {
        return;
    }
    let (pixels, stride) = state.pixels_mut();
    let stride = stride as usize;
    let total_h = pixels.len() / stride;
    let bytes_per_row = stride / 4;

    let clip_x_end = (clip_x0 + clip_w).min(bytes_per_row);

    let fg_r = fg.0;
    let fg_g = fg.1;
    let fg_b = fg.2;

    for sy in 0..glyph.height {
        let dy = dst_y + sy as isize;
        if dy < 0 {
            continue;
        }
        let dy = dy as usize;
        if dy >= total_h {
            break;
        }
        let src_row = &glyph.rgba[sy * glyph.width * 4..(sy + 1) * glyph.width * 4];
        for sx in 0..glyph.width {
            let dx = dst_x + sx as isize;
            if dx < 0 {
                continue;
            }
            let dx = dx as usize;
            if dx < clip_x0 || dx >= clip_x_end {
                continue;
            }
            let s = &src_row[sx * 4..sx * 4 + 4];
            let (sr, sg, sb, sa) = (s[0] as f32, s[1] as f32, s[2] as f32, s[3] as f32);
            if sa == 0.0 {
                continue;
            }
            let (out_r, out_g, out_b): (f32, f32, f32) = if glyph.has_color {
                // Colour glyph (emoji): use source colour as-is, source
                // is already premultiplied so we use over-blend with src.
                (sr, sg, sb)
            } else {
                // Mono glyph: tint pre-multiplied src by fg.
                let a = sa / 255.0;
                (fg_r * 255.0 * a, fg_g * 255.0 * a, fg_b * 255.0 * a)
            };
            let inv_a = 1.0_f32 - sa / 255.0;
            let off = dy * stride + dx * 4;
            // Existing dst is opaque BGRA.
            let db = pixels[off] as f32;
            let dg = pixels[off + 1] as f32;
            let dr = pixels[off + 2] as f32;
            let nr = (out_r + dr * inv_a).round().clamp(0.0_f32, 255.0_f32) as u8;
            let ng = (out_g + dg * inv_a).round().clamp(0.0_f32, 255.0_f32) as u8;
            let nb = (out_b + db * inv_a).round().clamp(0.0_f32, 255.0_f32) as u8;
            pixels[off] = nb;
            pixels[off + 1] = ng;
            pixels[off + 2] = nr;
            pixels[off + 3] = 0xff;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_renderer_starts_fresh() {
        let r = CpuRenderer::new();
        assert!(r.fresh);
        assert_eq!(r.cache_len(), 0);
        assert!(r.last_row_seqno.is_empty());
    }

    #[test]
    fn invalidate_clears_seqno_and_marks_fresh() {
        let mut r = CpuRenderer::new();
        r.fresh = false;
        r.last_row_seqno.insert(0, 7);
        r.invalidate_all();
        assert!(r.fresh);
        assert!(r.last_row_seqno.is_empty());
    }
}
