//! Theme derived from the user's color palette.
//!
//! Maps `config::color::Palette` fields to `ratatui::style::Style` objects so the
//! overlay automatically matches whatever color scheme the user has configured.
//!
//! --- weezterm remote features ---

use config::{Palette, RgbaColor};
use ratatui::style::{Color, Modifier, Style};

/// All styles used by the config overlay, derived from the user's palette.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Normal text (foreground on default background).
    pub text: Style,
    /// Dimmed/secondary text.
    pub text_dim: Style,
    /// Selected/highlighted item (e.g. current setting row).
    pub selected: Style,
    /// Active section label in the nav panel.
    pub section_active: Style,
    /// Inactive section label.
    pub section_inactive: Style,
    /// Setting value (not proposed).
    pub value: Style,
    /// Proposed value (user-modified).
    pub value_proposed: Style,
    /// Leader dots between name and value.
    pub dots: Style,
    /// Badge: [I] Inherited.
    pub badge_inherited: Style,
    /// Badge: [E] Editable.
    pub badge_editable: Style,
    /// Badge: [F] Fixed by Lua.
    pub badge_fixed: Style,
    /// Border/frame lines.
    pub border: Style,
    /// Header bar background + text.
    pub header: Style,
    /// Footer bar background + text.
    pub footer: Style,
    /// Footer key hints (bold keys).
    pub footer_key: Style,
    /// Selection background color (for composing per-span selected styles).
    pub selection_bg: Color,
    /// Details panel text.
    pub detail: Style,
    /// Details panel title line.
    pub detail_title: Style,
}

impl Theme {
    /// Derive a theme from the user's resolved palette.
    ///
    /// Falls back to sensible ANSI defaults for any `None` fields.
    pub fn from_palette(palette: &Palette) -> Self {
        let fg = palette_to_color(palette.foreground.as_ref(), Color::White);
        let bg = palette_to_color(palette.background.as_ref(), Color::Black);
        let sel_fg = palette_to_color(palette.selection_fg.as_ref(), Color::Black);
        let sel_bg = palette_to_color(palette.selection_bg.as_ref(), Color::LightCyan);
        let border_color = palette_to_color(palette.split.as_ref(), Color::DarkGray);

        // Use a subtly contrasting background for header/footer bars
        // by blending the palette background toward the foreground slightly
        let header_bg = blend_toward(bg, fg, 0.15);
        let header_fg = fg;

        // Extract ANSI colors from palette (with fallbacks)
        let ansi = |idx: usize, fallback: Color| -> Color {
            palette
                .ansi
                .as_ref()
                .and_then(|a: &[RgbaColor; 8]| a.get(idx))
                .map(rgba_to_color)
                .unwrap_or(fallback)
        };
        let bright = |idx: usize, fallback: Color| -> Color {
            palette
                .brights
                .as_ref()
                .and_then(|a: &[RgbaColor; 8]| a.get(idx))
                .map(rgba_to_color)
                .unwrap_or(fallback)
        };

        let dim_fg = ansi(7, Color::Gray); // silver/grey
        let green = ansi(2, Color::Green);
        let yellow = ansi(3, Color::Yellow);
        let cyan = bright(6, Color::LightCyan);
        let bright_yellow = bright(3, Color::LightYellow);

        Self {
            text: Style::new().fg(fg),
            text_dim: Style::new().fg(dim_fg),
            selected: Style::new()
                .fg(sel_fg)
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            section_active: Style::new()
                .fg(sel_fg)
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            section_inactive: Style::new().fg(dim_fg),
            value: Style::new().fg(cyan),
            value_proposed: Style::new().fg(bright_yellow).add_modifier(Modifier::BOLD),
            dots: Style::new().fg(Color::DarkGray),
            badge_inherited: Style::new().fg(dim_fg),
            badge_editable: Style::new().fg(green),
            badge_fixed: Style::new().fg(yellow),
            border: Style::new().fg(border_color),
            header: Style::new()
                .fg(header_fg)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
            footer: Style::new().fg(header_fg).bg(header_bg),
            footer_key: Style::new()
                .fg(header_fg)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
            selection_bg: sel_bg,
            detail: Style::new().fg(dim_fg),
            detail_title: Style::new().fg(fg).add_modifier(Modifier::BOLD),
        }
    }
}

/// Convert an `Option<&RgbaColor>` to a ratatui `Color`, with fallback.
fn palette_to_color(rgba: Option<&RgbaColor>, fallback: Color) -> Color {
    rgba.map(rgba_to_color).unwrap_or(fallback)
}

/// Convert an `RgbaColor` to a ratatui `Color::Rgb`.
fn rgba_to_color(rgba: &RgbaColor) -> Color {
    let t = **rgba; // deref to SrgbaTuple
    let r = (t.0 * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (t.1 * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (t.2 * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::Rgb(r, g, b)
}

/// Blend color `a` toward color `b` by `factor` (0.0 = pure a, 1.0 = pure b).
fn blend_toward(a: Color, b: Color, factor: f32) -> Color {
    let (ar, ag, ab) = color_to_rgb(a);
    let (br, bg, bb) = color_to_rgb(b);
    let mix = |a: u8, b: u8| -> u8 {
        let v = a as f32 * (1.0 - factor) + b as f32 * factor;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// Extract RGB components from a ratatui Color (best-effort for non-Rgb).
fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        Color::DarkGray => (128, 128, 128),
        Color::Gray => (192, 192, 192),
        _ => (128, 128, 128),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_rgba_to_color() {
        // White
        let white = RgbaColor::from((255u8, 255u8, 255u8));
        assert_eq!(rgba_to_color(&white), Color::Rgb(255, 255, 255));

        // Black
        let black = RgbaColor::from((0u8, 0u8, 0u8));
        assert_eq!(rgba_to_color(&black), Color::Rgb(0, 0, 0));

        // Mid-range
        let mid = RgbaColor::from((128u8, 64u8, 32u8));
        assert_eq!(rgba_to_color(&mid), Color::Rgb(128, 64, 32));
    }

    #[test]
    fn test_theme_from_empty_palette() {
        let palette = Palette::default();
        let theme = Theme::from_palette(&palette);
        // Should produce valid styles without panicking (all fallbacks)
        assert_eq!(theme.text.fg, Some(Color::White));
        assert_eq!(theme.border.fg, Some(Color::DarkGray));
    }

    #[test]
    fn test_theme_from_palette_with_colors() {
        let mut palette = Palette::default();
        palette.foreground = Some(RgbaColor::from((200u8, 200u8, 200u8)));
        palette.background = Some(RgbaColor::from((30u8, 30u8, 46u8)));
        palette.selection_fg = Some(RgbaColor::from((0u8, 0u8, 0u8)));
        palette.selection_bg = Some(RgbaColor::from((100u8, 180u8, 255u8)));

        let theme = Theme::from_palette(&palette);
        assert_eq!(theme.text.fg, Some(Color::Rgb(200, 200, 200)));
        assert_eq!(theme.selected.fg, Some(Color::Rgb(0, 0, 0)));
        assert_eq!(theme.selected.bg, Some(Color::Rgb(100, 180, 255)));
    }
}
