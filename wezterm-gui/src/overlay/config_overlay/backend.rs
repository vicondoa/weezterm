//! Custom Ratatui backend that wraps a `termwiz::terminal::Terminal`.
//!
//! This bridges Ratatui's widget system with WeezTerm's overlay terminal
//! abstraction, avoiding the `SystemTerminal` requirement of the stock
//! `TermwizBackend`.
//!
//! --- weezterm remote features ---

use ratatui::backend::{Backend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use std::io;
use termwiz::cell::{AttributeChange, Intensity, Underline};
use termwiz::color::{ColorAttribute, SrgbaTuple};
use termwiz::surface::{Change, CursorVisibility, Position as TWPos};
use termwiz::terminal::Terminal;

/// A Ratatui backend wrapping any `termwiz::terminal::Terminal`.
pub struct TermwizOverlayBackend<T: Terminal> {
    terminal: T,
    width: u16,
    height: u16,
}

impl<T: Terminal> TermwizOverlayBackend<T> {
    pub fn new(mut terminal: T) -> io::Result<Self> {
        let screen = terminal
            .get_screen_size()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(Self {
            terminal,
            width: screen.cols as u16,
            height: screen.rows as u16,
        })
    }

    /// Get a mutable reference to the underlying terminal for input polling.
    pub fn terminal_mut(&mut self) -> &mut T {
        &mut self.terminal
    }

    /// Re-query the terminal size (call after a resize event).
    pub fn refresh_size(&mut self) -> io::Result<()> {
        let screen = self
            .terminal
            .get_screen_size()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.width = screen.cols as u16;
        self.height = screen.rows as u16;
        Ok(())
    }
}

impl<T: Terminal> Backend for TermwizOverlayBackend<T> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut changes: Vec<Change> = Vec::new();
        let mut last_x: Option<u16> = None;
        let mut last_y: Option<u16> = None;

        for (x, y, cell) in content {
            // Position cursor if not contiguous
            if last_y != Some(y) || last_x.map_or(true, |lx| lx + 1 != x) {
                changes.push(Change::CursorPosition {
                    x: TWPos::Absolute(x as usize),
                    y: TWPos::Absolute(y as usize),
                });
            }
            last_x = Some(x);
            last_y = Some(y);

            // Convert ratatui style to termwiz attributes
            let fg = ratatui_color_to_termwiz(cell.fg);
            let bg = ratatui_color_to_termwiz(cell.bg);

            changes.push(Change::Attribute(AttributeChange::Foreground(fg)));
            changes.push(Change::Attribute(AttributeChange::Background(bg)));
            changes.push(Change::Attribute(AttributeChange::Intensity(
                if cell.modifier.contains(ratatui::style::Modifier::BOLD) {
                    Intensity::Bold
                } else if cell.modifier.contains(ratatui::style::Modifier::DIM) {
                    Intensity::Half
                } else {
                    Intensity::Normal
                },
            )));
            changes.push(Change::Attribute(AttributeChange::Italic(
                cell.modifier.contains(ratatui::style::Modifier::ITALIC),
            )));
            changes.push(Change::Attribute(AttributeChange::Underline(
                if cell.modifier.contains(ratatui::style::Modifier::UNDERLINED) {
                    Underline::Single
                } else {
                    Underline::None
                },
            )));
            changes.push(Change::Attribute(AttributeChange::Reverse(
                cell.modifier.contains(ratatui::style::Modifier::REVERSED),
            )));

            changes.push(Change::Text(cell.symbol().to_string()));
        }

        self.terminal
            .render(&changes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.terminal
            .render(&[Change::CursorVisibility(CursorVisibility::Hidden)])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.terminal
            .render(&[Change::CursorVisibility(CursorVisibility::Visible)])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::new(0, 0))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, pos: P) -> io::Result<()> {
        let pos = pos.into();
        self.terminal
            .render(&[Change::CursorPosition {
                x: TWPos::Absolute(pos.x as usize),
                y: TWPos::Absolute(pos.y as usize),
            }])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn clear(&mut self) -> io::Result<()> {
        self.terminal
            .render(&[Change::ClearScreen(ColorAttribute::Default)])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(self.width, self.height))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.terminal
            .flush()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: Size::new(self.width, self.height),
            pixels: Size::new(0, 0),
        })
    }
}

/// Convert a ratatui `Color` to a termwiz `ColorAttribute`.
fn ratatui_color_to_termwiz(color: ratatui::style::Color) -> ColorAttribute {
    use ratatui::style::Color;
    match color {
        Color::Reset => ColorAttribute::Default,
        Color::Black => ColorAttribute::PaletteIndex(0),
        Color::Red => ColorAttribute::PaletteIndex(1),
        Color::Green => ColorAttribute::PaletteIndex(2),
        Color::Yellow => ColorAttribute::PaletteIndex(3),
        Color::Blue => ColorAttribute::PaletteIndex(4),
        Color::Magenta => ColorAttribute::PaletteIndex(5),
        Color::Cyan => ColorAttribute::PaletteIndex(6),
        Color::Gray => ColorAttribute::PaletteIndex(7),
        Color::DarkGray => ColorAttribute::PaletteIndex(8),
        Color::LightRed => ColorAttribute::PaletteIndex(9),
        Color::LightGreen => ColorAttribute::PaletteIndex(10),
        Color::LightYellow => ColorAttribute::PaletteIndex(11),
        Color::LightBlue => ColorAttribute::PaletteIndex(12),
        Color::LightMagenta => ColorAttribute::PaletteIndex(13),
        Color::LightCyan => ColorAttribute::PaletteIndex(14),
        Color::White => ColorAttribute::PaletteIndex(15),
        Color::Rgb(r, g, b) => ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0,
        )),
        Color::Indexed(idx) => ColorAttribute::PaletteIndex(idx),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn test_ratatui_color_to_termwiz_ansi() {
        assert_eq!(
            ratatui_color_to_termwiz(Color::Reset),
            ColorAttribute::Default
        );
        assert_eq!(
            ratatui_color_to_termwiz(Color::Black),
            ColorAttribute::PaletteIndex(0)
        );
        assert_eq!(
            ratatui_color_to_termwiz(Color::White),
            ColorAttribute::PaletteIndex(15)
        );
        assert_eq!(
            ratatui_color_to_termwiz(Color::Indexed(42)),
            ColorAttribute::PaletteIndex(42)
        );
    }

    #[test]
    fn test_ratatui_color_to_termwiz_rgb() {
        match ratatui_color_to_termwiz(Color::Rgb(255, 128, 0)) {
            ColorAttribute::TrueColorWithDefaultFallback(c) => {
                assert!((c.0 - 1.0).abs() < 0.01);
                assert!((c.1 - 0.502).abs() < 0.01);
                assert!((c.2 - 0.0).abs() < 0.01);
            }
            other => panic!("Expected TrueColor, got {:?}", other),
        }
    }
}
