// --- weezterm remote features ---
use std::error::Error;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, mem};

use smithay_client_toolkit::compositor::SurfaceData;
use smithay_client_toolkit::reexports::client::protocol::wl_shm;
use smithay_client_toolkit::reexports::client::protocol::wl_subsurface::WlSubsurface;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Dispatch, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::csd_frame::{
    DecorationsFrame, FrameAction, FrameClick, WindowManagerCapabilities, WindowState,
};
use smithay_client_toolkit::seat::pointer::CursorIcon;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::subcompositor::{SubcompositorState, SubsurfaceData};
use tiny_skia::{ColorU8, PixmapMut, PixmapPaint, PixmapRef, Transform};
use wayland_backend::client::ObjectId;
use wezterm_font::{FontConfiguration, FontMetrics, GlyphInfo, RasterizedGlyph};

const HEADER_SIZE: u32 = 24;

const BTN_ICON_COLOR: u32 = 0xFFCCCCCC;
const BTN_HOVER_BG: u32 = 0xFF808080;
const PRIMARY_COLOR_ACTIVE: u32 = 0xFF3A3A3A;
const PRIMARY_COLOR_INACTIVE: u32 = 0xFF242424;

#[derive(Debug)]
pub struct TitleBarFrame<State> {
    parent: WlSurface,
    state: WindowState,
    wm_capabilities: WindowManagerCapabilities,
    dirty: bool,
    mouse_location: Location,
    mouse_coords: (i32, i32),
    render_data: Option<FrameRenderData>,
    should_sync: bool,
    scale_factor: f64,
    queue_handle: QueueHandle<State>,
    pool: SlotPool,
    subcompositor: Arc<SubcompositorState>,
    buttons: [Option<UIButton>; 3],
    font_config: TitleFontConfig,
    title: String,
    shaped_title: Option<ShapedTitle>,
    _state: PhantomData<State>,
}

#[repr(transparent)]
struct TitleFontConfig(Rc<FontConfiguration>);

impl fmt::Debug for TitleFontConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TitleFontConfig")
    }
}

impl Deref for TitleFontConfig {
    type Target = FontConfiguration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct ShapedTitle {
    title: String,
    glyphs: Vec<ShapedGlyph>,
    metrics: FontMetrics,
    is_active: bool,
    dpi: usize,
}

impl fmt::Debug for ShapedTitle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShapedTitle")
            .field("title", &self.title)
            .field("glyph_count", &self.glyphs.len())
            .field("metrics", &self.metrics)
            .field("is_active", &self.is_active)
            .field("dpi", &self.dpi)
            .finish()
    }
}

struct ShapedGlyph {
    info: GlyphInfo,
    glyph: RasterizedGlyph,
}

fn title_tail_start(advances: &[f64], available_width: f64) -> usize {
    if advances.iter().sum::<f64>() <= available_width {
        return 0;
    }

    let mut tail_width = 0.;
    let mut start = advances.len();
    for (idx, advance) in advances.iter().enumerate().rev() {
        if tail_width + advance > available_width {
            break;
        }
        tail_width += advance;
        start = idx;
    }
    start
}

impl<State> TitleBarFrame<State>
where
    State: Dispatch<WlSurface, SurfaceData> + Dispatch<WlSubsurface, SubsurfaceData> + 'static,
{
    pub fn new(
        parent: &impl WaylandSurface,
        shm: &Shm,
        subcompositor: Arc<SubcompositorState>,
        queue_handle: QueueHandle<State>,
        font_config: Rc<FontConfiguration>,
    ) -> Result<Self, Box<dyn Error>> {
        let parent = parent.wl_surface().clone();
        let pool = SlotPool::new(1, shm)?;
        let render_data = Some(FrameRenderData::new(&parent, &subcompositor, &queue_handle));
        let wm_capabilities = WindowManagerCapabilities::all();

        Ok(Self {
            parent,
            state: WindowState::empty(),
            wm_capabilities,
            dirty: true,
            mouse_location: Location::None,
            mouse_coords: (0, 0),
            render_data,
            should_sync: true,
            scale_factor: 1.0,
            queue_handle,
            pool,
            subcompositor,
            buttons: Self::supported_buttons(wm_capabilities),
            font_config: TitleFontConfig(font_config),
            title: String::new(),
            shaped_title: None,
            _state: PhantomData,
        })
    }

    fn reshape_title(&mut self, is_active: bool) -> Option<()> {
        if self.title.is_empty() {
            self.shaped_title.take();
            return Some(());
        }

        if let Some(existing) = self.shaped_title.as_ref() {
            if existing.title == self.title
                && existing.is_active == is_active
                && existing.dpi == self.font_config.get_dpi()
            {
                return Some(());
            }
        }

        let font = self.font_config.title_font().ok()?;
        let metrics = font.metrics();
        let infos = font
            .shape(
                &self.title,
                || {},
                |_| {},
                None,
                wezterm_bidi::Direction::LeftToRight,
                None,
                None,
            )
            .ok()?;
        let color_level = if is_active { 0xcc } else { 0x99 };
        let mut glyphs = vec![];

        for info in infos {
            if let Ok(mut glyph) = font.rasterize_glyph(info.glyph_pos, info.font_idx) {
                if let Some(mut data) =
                    PixmapMut::from_bytes(&mut glyph.data, glyph.width as u32, glyph.height as u32)
                {
                    for pixel in data.pixels_mut() {
                        let color = pixel.demultiply();
                        let (red, green, blue, alpha) =
                            (color.red(), color.green(), color.blue(), color.alpha());
                        if glyph.has_color {
                            *pixel = ColorU8::from_rgba(blue, green, red, alpha).premultiply();
                        } else {
                            *pixel = ColorU8::from_rgba(
                                ((blue as f32 / 255.) * color_level as f32) as u8,
                                ((green as f32 / 255.) * color_level as f32) as u8,
                                ((red as f32 / 255.) * color_level as f32) as u8,
                                alpha,
                            )
                            .premultiply();
                        }
                    }
                }
                glyphs.push(ShapedGlyph { info, glyph });
            }
        }

        self.shaped_title.replace(ShapedTitle {
            title: self.title.clone(),
            glyphs,
            metrics,
            is_active,
            dpi: self.font_config.get_dpi(),
        });
        Some(())
    }

    fn draw_title(
        shaped_title: Option<&ShapedTitle>,
        canvas: &mut [u8],
        width: u32,
        scale: u32,
        button_count: usize,
    ) {
        let Some(shaped) = shaped_title else {
            return;
        };
        let scaled_width = width * scale;
        let scaled_height = HEADER_SIZE * scale;
        let Some(mut pixmap) = PixmapMut::from_bytes(canvas, scaled_width, scaled_height) else {
            return;
        };
        let mut x = f64::from(8 * scale);
        let reserved = (button_count as u32 + 1) * HEADER_SIZE * scale;
        let limit = scaled_width.saturating_sub(reserved) as f64;
        let paint = PixmapPaint::default();
        let baseline =
            ((f64::from(scaled_height) + shaped.metrics.cell_height.get()) / 2.).round() as i32;
        let advances = shaped
            .glyphs
            .iter()
            .map(|item| item.info.x_advance.get())
            .collect::<Vec<_>>();
        let start = title_tail_start(&advances, (limit - x).max(0.));

        for item in &shaped.glyphs[start..] {
            if let Some(data) = PixmapRef::from_bytes(
                &item.glyph.data,
                item.glyph.width as u32,
                item.glyph.height as u32,
            ) {
                pixmap.draw_pixmap(
                    (x + item.info.x_offset.get() + item.glyph.bearing_x.get()) as i32,
                    baseline
                        + (shaped.metrics.descender - (item.info.y_offset + item.glyph.bearing_y))
                            .get() as i32,
                    data,
                    &paint,
                    Transform::identity(),
                    None,
                );
            }

            x += item.info.x_advance.get();
            if x >= limit {
                break;
            }
        }
    }

    fn supported_buttons(wm_capabilities: WindowManagerCapabilities) -> [Option<UIButton>; 3] {
        let maximize = wm_capabilities
            .contains(WindowManagerCapabilities::MAXIMIZE)
            .then_some(UIButton::Maximize);
        let minimize = wm_capabilities
            .contains(WindowManagerCapabilities::MINIMIZE)
            .then_some(UIButton::Minimize);
        [Some(UIButton::Close), maximize, minimize]
    }

    fn find_button(buttons: &[Option<UIButton>], x: f64, y: f64, width: u32) -> Location {
        for (idx, &button) in buttons.iter().flatten().enumerate() {
            let idx = idx as u32;
            if width >= (idx + 1) * HEADER_SIZE
                && x >= f64::from(width - (idx + 1) * HEADER_SIZE)
                && x <= f64::from(width - idx * HEADER_SIZE)
                && y <= f64::from(HEADER_SIZE)
                && y >= 0.0
            {
                return Location::Button(button);
            }
        }

        Location::Head
    }

    fn part_index_for_surface(&self, surface_id: &ObjectId) -> Option<usize> {
        self.render_data
            .as_ref()
            .and_then(|data| (&data.header.surface.id() == surface_id).then_some(0))
    }

    fn draw_buttons(
        buttons: &[Option<UIButton>],
        canvas: &mut [u8],
        width: u32,
        scale: u32,
        is_active: bool,
        mouse_location: &Location,
    ) {
        let scale = scale as usize;
        for (idx, &button) in buttons.iter().flatten().enumerate() {
            if width >= (idx + 1) as u32 * HEADER_SIZE {
                if is_active && mouse_location == &Location::Button(button) {
                    Self::draw_button(
                        canvas,
                        idx * HEADER_SIZE as usize,
                        scale,
                        width as usize,
                        BTN_HOVER_BG.to_le_bytes(),
                    );
                }
                Self::draw_icon(
                    canvas,
                    width as usize,
                    idx * HEADER_SIZE as usize,
                    scale,
                    BTN_ICON_COLOR.to_le_bytes(),
                    button,
                );
            }
        }
    }

    fn draw_button(
        canvas: &mut [u8],
        x_offset: usize,
        scale: usize,
        width: usize,
        btn_color: [u8; 4],
    ) {
        let height = HEADER_SIZE as usize;
        let x_start = width - height - x_offset;
        for y in 0..height * scale {
            let start = (x_start + y * width) * 4 * scale;
            let end = (x_start + y * width + height) * scale * 4;
            for pixel in canvas[start..end].chunks_exact_mut(4) {
                pixel[0] = btn_color[0];
                pixel[1] = btn_color[1];
                pixel[2] = btn_color[2];
                pixel[3] = btn_color[3];
            }
        }
    }

    fn draw_icon(
        canvas: &mut [u8],
        width: usize,
        x_offset: usize,
        scale: usize,
        icon_color: [u8; 4],
        icon: UIButton,
    ) {
        let height = HEADER_SIZE as usize;
        let scaled_height = scale * height;
        let x_start = width - height - x_offset;

        match icon {
            UIButton::Close => {
                for y in scaled_height / 4..3 * scaled_height / 4 {
                    let line = &mut canvas[(x_start + y * width + height / 4) * 4 * scale
                        ..(x_start + y * width + 3 * height / 4) * 4 * scale];
                    for pixel in line.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                }
            }
            UIButton::Maximize => {
                for y in 2 * scaled_height / 8..3 * scaled_height / 8 {
                    let line = &mut canvas[(x_start + y * width + height / 4) * 4 * scale
                        ..(x_start + y * width + 3 * height / 4) * 4 * scale];
                    for pixel in line.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                }
                for y in 3 * scaled_height / 8..5 * scaled_height / 8 {
                    let left = &mut canvas[(x_start + y * width + 2 * height / 8) * 4 * scale
                        ..(x_start + y * width + 3 * height / 8) * 4 * scale];
                    for pixel in left.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                    let right = &mut canvas[(x_start + y * width + 5 * height / 8) * 4 * scale
                        ..(x_start + y * width + 6 * height / 8) * 4 * scale];
                    for pixel in right.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                }
                for y in 5 * scaled_height / 8..6 * scaled_height / 8 {
                    let line = &mut canvas[(x_start + y * width + height / 4) * 4 * scale
                        ..(x_start + y * width + 3 * height / 4) * 4 * scale];
                    for pixel in line.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                }
            }
            UIButton::Minimize => {
                for y in 5 * scaled_height / 8..3 * scaled_height / 4 {
                    let line = &mut canvas[(x_start + y * width + height / 4) * 4 * scale
                        ..(x_start + y * width + 3 * height / 4) * 4 * scale];
                    for pixel in line.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&icon_color);
                    }
                }
            }
        }
    }
}

impl<State> DecorationsFrame for TitleBarFrame<State>
where
    State: Dispatch<WlSurface, SurfaceData> + Dispatch<WlSubsurface, SubsurfaceData> + 'static,
{
    fn on_click(
        &mut self,
        _timestamp: Duration,
        click: FrameClick,
        pressed: bool,
    ) -> Option<FrameAction> {
        if click == FrameClick::Alternate {
            return if self.mouse_location != Location::Head
                || !self
                    .wm_capabilities
                    .contains(WindowManagerCapabilities::WINDOW_MENU)
            {
                None
            } else {
                Some(FrameAction::ShowMenu(
                    self.mouse_coords.0,
                    self.mouse_coords.1 - HEADER_SIZE as i32,
                ))
            };
        }

        match self.mouse_location {
            Location::Head if pressed => Some(FrameAction::Move),
            Location::Button(UIButton::Close) if !pressed => Some(FrameAction::Close),
            Location::Button(UIButton::Minimize) if !pressed => Some(FrameAction::Minimize),
            Location::Button(UIButton::Maximize)
                if !pressed && !self.state.contains(WindowState::MAXIMIZED) =>
            {
                Some(FrameAction::Maximize)
            }
            Location::Button(UIButton::Maximize)
                if !pressed && self.state.contains(WindowState::MAXIMIZED) =>
            {
                Some(FrameAction::UnMaximize)
            }
            _ => None,
        }
    }

    fn click_point_moved(
        &mut self,
        _timestamp: Duration,
        surface_id: &ObjectId,
        x: f64,
        y: f64,
    ) -> Option<CursorIcon> {
        self.part_index_for_surface(surface_id)?;
        let old_location = self.mouse_location;
        self.mouse_coords = (x as i32, y as i32);
        let width = self.render_data.as_ref()?.header.width;
        self.mouse_location = Self::find_button(&self.buttons, x, y, width);
        self.dirty |= (matches!(old_location, Location::Button(_))
            || matches!(self.mouse_location, Location::Button(_)))
            && old_location != self.mouse_location;
        Some(CursorIcon::Default)
    }

    fn click_point_left(&mut self) {
        self.mouse_location = Location::None;
        self.dirty = true;
    }

    fn update_state(&mut self, state: WindowState) {
        let difference = self.state.symmetric_difference(state);
        self.state = state;
        self.dirty |= !difference
            .intersection(WindowState::ACTIVATED | WindowState::FULLSCREEN | WindowState::MAXIMIZED)
            .is_empty();
    }

    fn update_wm_capabilities(&mut self, wm_capabilities: WindowManagerCapabilities) {
        self.dirty |= self.wm_capabilities != wm_capabilities;
        self.wm_capabilities = wm_capabilities;
        self.buttons = Self::supported_buttons(wm_capabilities);
    }

    fn resize(&mut self, width: NonZeroU32, _height: NonZeroU32) {
        let render_data = self
            .render_data
            .as_mut()
            .expect("trying to resize hidden frame");
        render_data.header.width = width.get();
        self.dirty = true;
        self.should_sync = true;
    }

    fn set_scaling_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
        self.dirty = true;
        self.should_sync = true;
    }

    fn location(&self) -> (i32, i32) {
        if self.state.contains(WindowState::FULLSCREEN) || self.is_hidden() {
            (0, 0)
        } else {
            (0, -(HEADER_SIZE as i32))
        }
    }

    fn subtract_borders(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> (Option<NonZeroU32>, Option<NonZeroU32>) {
        if self.state.contains(WindowState::FULLSCREEN) || self.is_hidden() {
            (Some(width), Some(height))
        } else {
            (
                Some(width),
                NonZeroU32::new(height.get().saturating_sub(HEADER_SIZE)),
            )
        }
    }

    fn add_borders(&self, width: u32, height: u32) -> (u32, u32) {
        if self.state.contains(WindowState::FULLSCREEN) || self.is_hidden() {
            (width, height)
        } else {
            (width, height + HEADER_SIZE)
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn set_hidden(&mut self, hidden: bool) {
        if self.is_hidden() == hidden {
            return;
        }

        if hidden {
            self.render_data = None;
        } else {
            let _ = self.pool.resize(1);
            self.render_data = Some(FrameRenderData::new(
                &self.parent,
                &self.subcompositor,
                &self.queue_handle,
            ));
        }
        self.dirty = true;
        self.should_sync = true;
    }

    fn is_hidden(&self) -> bool {
        self.render_data.is_none()
    }

    fn set_resizable(&mut self, _resizable: bool) {}

    fn draw(&mut self) -> bool {
        if self.render_data.is_none() {
            return false;
        }
        let is_active = self.state.contains(WindowState::ACTIVATED);
        self.reshape_title(is_active);
        let render_data = match self.render_data.as_mut() {
            Some(render_data) => render_data,
            None => return false,
        };

        self.dirty = false;
        let should_sync = mem::take(&mut self.should_sync);

        if self.state.contains(WindowState::FULLSCREEN) {
            render_data.header.surface.attach(None, 0, 0);
            render_data.header.surface.commit();
            return should_sync;
        }

        let fill_color = if is_active {
            PRIMARY_COLOR_ACTIVE
        } else {
            PRIMARY_COLOR_INACTIVE
        }
        .to_le_bytes();
        let scale = self.scale_factor.ceil() as i32;
        let width = render_data.header.width;
        let height = HEADER_SIZE;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32 * scale,
            height as i32 * scale,
            width as i32 * 4 * scale,
            wl_shm::Format::Argb8888,
        ) {
            Ok((buffer, canvas)) => (buffer, canvas),
            Err(_) => return should_sync,
        };

        for pixel in canvas.chunks_exact_mut(4) {
            pixel.copy_from_slice(&fill_color);
        }

        Self::draw_title(
            self.shaped_title.as_ref(),
            canvas,
            width,
            scale as u32,
            self.buttons.iter().flatten().count(),
        );
        Self::draw_buttons(
            &self.buttons,
            canvas,
            width,
            scale as u32,
            is_active,
            &self.mouse_location,
        );

        render_data.header.surface.set_buffer_scale(scale);
        if should_sync {
            render_data.header.subsurface.set_sync();
        } else {
            render_data.header.subsurface.set_desync();
        }
        render_data
            .header
            .subsurface
            .set_position(render_data.header.pos.0, render_data.header.pos.1);

        if let Err(err) = buffer.attach_to(&render_data.header.surface) {
            log::error!("failed to attach titlebar buffer: {err}");
            return should_sync;
        }
        if render_data.header.surface.version() >= 4 {
            render_data
                .header
                .surface
                .damage_buffer(0, 0, i32::MAX, i32::MAX);
        } else {
            render_data.header.surface.damage(0, 0, i32::MAX, i32::MAX);
        }
        render_data.header.surface.commit();

        should_sync
    }

    fn set_title(&mut self, title: impl Into<String>) {
        let title = title.into();
        if self.title != title {
            self.title = title;
            self.shaped_title.take();
            self.dirty = true;
        }
    }
}

#[derive(Debug)]
struct FrameRenderData {
    header: FramePart,
}

impl FrameRenderData {
    fn new<State>(
        parent: &WlSurface,
        subcompositor: &SubcompositorState,
        queue_handle: &QueueHandle<State>,
    ) -> Self
    where
        State: Dispatch<WlSurface, SurfaceData> + Dispatch<WlSubsurface, SubsurfaceData> + 'static,
    {
        Self {
            header: FramePart::new(
                subcompositor.create_subsurface(parent.clone(), queue_handle),
                HEADER_SIZE,
                (0, -(HEADER_SIZE as i32)),
            ),
        }
    }
}

#[derive(Debug)]
struct FramePart {
    subsurface: WlSubsurface,
    surface: WlSurface,
    width: u32,
    pos: (i32, i32),
}

impl FramePart {
    fn new(surfaces: (WlSubsurface, WlSurface), width: u32, pos: (i32, i32)) -> Self {
        let (subsurface, surface) = surfaces;
        subsurface.set_sync();
        Self {
            subsurface,
            surface,
            width,
            pos,
        }
    }
}

impl Drop for FramePart {
    fn drop(&mut self) {
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Location {
    None,
    Head,
    Button(UIButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UIButton {
    Close,
    Maximize,
    Minimize,
}

#[cfg(test)]
mod test {
    use super::title_tail_start;

    #[test]
    fn title_overflow_keeps_the_trailing_glyphs() {
        assert_eq!(title_tail_start(&[4., 4., 4., 4.], 16.), 0);
        assert_eq!(title_tail_start(&[4., 4., 4., 4.], 9.), 2);
        assert_eq!(title_tail_start(&[12., 4.], 4.), 1);
    }
}
// --- end weezterm remote features ---
