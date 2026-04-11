//! Rendering helpers for the config overlay TUI.
//!
//! Renders a centered, boxed two-panel layout with section nav on the left
//! and settings + details on the right. Capped at 86×30 to avoid sprawling
//! across maximized terminals.
//!
//! --- weezterm remote features ---

use super::{FieldStatus, OverlayState, Panel, SettingRow};
use crate::overlay::config_overlay::data::FieldKind;
use termwiz::cell::{AttributeChange, Intensity};
use termwiz::color::{AnsiColor, ColorAttribute};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;

const MAX_WIDTH: usize = 86;
const MAX_HEIGHT: usize = 30;
const SECTION_W: usize = 18;
// Overhead rows: top_border(1) + title(1) + search(1) + sep(1) +
//   detail_sep(1) + detail(3) + sep(1) + footer(1) + bottom(1) = 11
const OVERHEAD: usize = 11;
const DETAIL_ROWS: usize = 3;

/// Computed layout geometry (shared with mouse handler via `compute_layout`).
pub struct Layout {
    pub left_pad: usize,
    pub top_pad: usize,
    pub width: usize,
    #[allow(dead_code)]
    pub height: usize,
    pub section_w: usize,
    pub settings_w: usize,
    pub body_start_y: usize,
    pub body_rows: usize,
}

pub fn compute_layout(cols: usize, rows: usize) -> Layout {
    let width = cols.min(MAX_WIDTH).max(50);
    let height = rows.min(MAX_HEIGHT).max(15);
    let left_pad = cols.saturating_sub(width) / 2;
    let top_pad = rows.saturating_sub(height) / 2;
    let inner = width.saturating_sub(2);
    let section_w = SECTION_W.min(inner.saturating_sub(10));
    let settings_w = inner.saturating_sub(section_w + 1);
    let body_rows = height.saturating_sub(OVERHEAD).max(3);
    let body_start_y = 4; // after top_border, title, search, separator
    Layout {
        left_pad,
        top_pad,
        width,
        height,
        section_w,
        settings_w,
        body_start_y,
        body_rows,
    }
}

/// Render the complete overlay frame.
pub fn render_frame(term: &mut impl Terminal, state: &OverlayState) -> anyhow::Result<()> {
    let screen = term.get_screen_size()?;
    let ly = compute_layout(screen.cols, screen.rows);
    let pad = " ".repeat(ly.left_pad);
    let inner_w = ly.width.saturating_sub(2);

    let mut c = vec![
        Change::ClearScreen(Default::default()),
        Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(0),
        },
    ];

    // Blank lines for vertical centering
    for _ in 0..ly.top_pad {
        c.push(Change::Text("\r\n".into()));
    }

    // ── Top border ───────────────────────────────────────────────────
    reset(&mut c);
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!(
        "{}┌{}┐\r\n",
        pad,
        "─".repeat(inner_w)
    )));

    // ── Title row ────────────────────────────────────────────────────
    let section_name = state.current_section().display_name();
    let title = format!(" Configure WezTerm ── {}", section_name);
    let modified = if state.dirty { " [modified] " } else { "" };
    let title_pad = inner_w.saturating_sub(title.len() + modified.len()).max(0);

    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!("{}│", pad)));
    bg(&mut c, AnsiColor::Navy);
    fg(&mut c, AnsiColor::White);
    bold(&mut c, true);
    c.push(Change::Text(title.clone()));
    bold(&mut c, false);
    c.push(Change::Text(" ".repeat(title_pad)));
    if state.dirty {
        fg(&mut c, AnsiColor::Yellow);
        c.push(Change::Text(modified.into()));
    }
    reset_bg(&mut c);
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text("│\r\n".into()));

    // ── Search row ───────────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!("{}│", pad)));
    bg(&mut c, AnsiColor::Navy);
    let search_text = if state.filter_active || !state.filter.is_empty() {
        fg(&mut c, AnsiColor::White);
        if state.filter_active {
            format!(" / {}▏", state.filter)
        } else {
            format!(" / {}", state.filter)
        }
    } else {
        fg(&mut c, AnsiColor::Grey);
        " / search…".into()
    };
    c.push(Change::Text(search_text.clone()));
    let search_edit_text = if let Some(ref edit) = state.inline_edit {
        fg(&mut c, AnsiColor::Aqua);
        bold(&mut c, true);
        let t = format!("    Edit: {}▏", edit.buffer);
        c.push(Change::Text(t.clone()));
        bold(&mut c, false);
        t
    } else {
        String::new()
    };
    let search_pad = inner_w
        .saturating_sub(search_text.chars().count() + search_edit_text.chars().count())
        .max(0);
    c.push(Change::Text(" ".repeat(search_pad)));
    reset_bg(&mut c);
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text("│\r\n".into()));

    // ── Header separator ─────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!(
        "{}├{}┬{}┤\r\n",
        pad,
        "─".repeat(ly.section_w),
        "─".repeat(ly.settings_w)
    )));

    // ── Body rows ────────────────────────────────────────────────────
    let settings = state.visible_settings();
    let selected_row = settings.get(state.selected_setting).cloned();

    for row in 0..ly.body_rows {
        fg(&mut c, AnsiColor::Silver);
        c.push(Change::Text(format!("{}│", pad)));
        render_section_cell(&mut c, state, row, ly.section_w);
        fg(&mut c, AnsiColor::Silver);
        c.push(Change::Text("│".into()));
        render_setting_cell(&mut c, state, &settings, row, ly.settings_w);
        fg(&mut c, AnsiColor::Silver);
        c.push(Change::Text("│\r\n".into()));
    }

    // ── Detail separator ─────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!(
        "{}│{}├{}┤\r\n",
        pad,
        " ".repeat(ly.section_w),
        "─".repeat(ly.settings_w)
    )));

    // ── Detail rows ──────────────────────────────────────────────────
    for detail_row in 0..DETAIL_ROWS {
        fg(&mut c, AnsiColor::Silver);
        c.push(Change::Text(format!(
            "{}│{}",
            pad,
            " ".repeat(ly.section_w)
        )));
        c.push(Change::Text("│".into()));
        render_detail_row(&mut c, &selected_row, state, detail_row, ly.settings_w);
        fg(&mut c, AnsiColor::Silver);
        c.push(Change::Text("│\r\n".into()));
    }

    // ── Footer separator ─────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!(
        "{}├{}┴{}┤\r\n",
        pad,
        "─".repeat(ly.section_w),
        "─".repeat(ly.settings_w)
    )));

    // ── Footer ───────────────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!("{}│", pad)));
    bg(&mut c, AnsiColor::Navy);
    render_footer(&mut c, inner_w);
    reset_bg(&mut c);
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text("│\r\n".into()));

    // ── Bottom border ────────────────────────────────────────────────
    fg(&mut c, AnsiColor::Silver);
    c.push(Change::Text(format!(
        "{}└{}┘\r\n",
        pad,
        "─".repeat(inner_w)
    )));

    reset(&mut c);
    term.render(&c)?;
    term.flush()?;
    Ok(())
}

// ─── Section panel (left) ───────────────────────────────────────────────────

fn render_section_cell(c: &mut Vec<Change>, state: &OverlayState, row: usize, w: usize) {
    if row < state.sections.len() {
        let section = state.sections[row];
        let is_current = row == state.selected_section;
        let is_focused = state.active_panel == Panel::Sections && is_current;

        if is_focused {
            bg(c, AnsiColor::Blue);
            fg(c, AnsiColor::White);
            bold(c, true);
        } else if is_current {
            reset_bg(c);
            fg(c, AnsiColor::White);
            bold(c, true);
        } else {
            reset_bg(c);
            fg(c, AnsiColor::Grey);
            bold(c, false);
        }

        let prefix = if is_current { " ▸ " } else { "   " };
        let label = section.display_name();
        let text = format!("{}{}", prefix, label);
        c.push(Change::Text(pad_to(&text, w)));
        bold(c, false);
        reset_bg(c);
    } else {
        reset_bg(c);
        c.push(Change::Text(" ".repeat(w)));
    }
}

// ─── Settings panel (center) ────────────────────────────────────────────────

fn render_setting_cell(
    c: &mut Vec<Change>,
    state: &OverlayState,
    settings: &[SettingRow],
    row: usize,
    w: usize,
) {
    let idx = row + state.settings_scroll_offset;
    if idx >= settings.len() {
        reset_bg(c);
        c.push(Change::Text(" ".repeat(w)));
        return;
    }

    let setting = &settings[idx];
    let is_selected = idx == state.selected_setting && state.active_panel == Panel::Settings;

    // Selected row gets a highlight background
    if is_selected {
        bg(c, AnsiColor::Navy);
        fg(c, AnsiColor::White);
        bold(c, true);
    } else {
        reset_bg(c);
        bold(c, false);
    }

    let prefix = if is_selected { " ▸ " } else { "   " };
    let name = &setting.display_name;

    // Value + badge go on the right
    let value_text = setting
        .proposed_value
        .as_ref()
        .unwrap_or(&setting.current_value);
    let value_display = trunc(value_text, 14);
    let badge = match setting.status {
        FieldStatus::Inherited => "[I]",
        FieldStatus::Editable => "[E]",
        FieldStatus::FixedByLua => "[F]",
    };

    // Right side: " VALUE BADGE " = 1 + value_len + 1 + 3 + 1
    let right_len = 1 + value_display.len() + 1 + badge.len() + 1;
    let left_len = prefix.len() + name.len();
    let dots_len = w.saturating_sub(left_len + right_len + 1).max(1);

    // Emit prefix + name
    c.push(Change::Text(prefix.into()));
    if !is_selected {
        fg(c, AnsiColor::White);
    }
    c.push(Change::Text(trunc(
        name,
        w.saturating_sub(right_len + dots_len + prefix.len()),
    )));

    // Emit leader dots
    c.push(Change::Text(" ".into()));
    if is_selected {
        fg(c, AnsiColor::Silver);
    } else {
        fg(c, AnsiColor::Grey);
    }
    c.push(Change::Text("·".repeat(dots_len.saturating_sub(1))));
    c.push(Change::Text(" ".into()));

    // Emit value
    if setting.proposed_value.is_some() {
        fg(c, AnsiColor::Aqua);
    } else if is_selected {
        fg(c, AnsiColor::White);
    } else {
        fg(c, AnsiColor::Silver);
    }
    c.push(Change::Text(value_display));

    // Emit badge
    c.push(Change::Text(" ".into()));
    match setting.status {
        FieldStatus::Inherited => fg(c, AnsiColor::Grey),
        FieldStatus::Editable => fg(c, AnsiColor::Green),
        FieldStatus::FixedByLua => fg(c, AnsiColor::Yellow),
    }
    c.push(Change::Text(badge.into()));
    c.push(Change::Text(" ".into()));

    bold(c, false);
    reset_bg(c);
}

// ─── Detail rows (bottom-right) ─────────────────────────────────────────────

fn render_detail_row(
    c: &mut Vec<Change>,
    selected: &Option<SettingRow>,
    state: &OverlayState,
    row: usize,
    w: usize,
) {
    let text = match (selected, row) {
        (Some(r), 0) => {
            let field_def = state.field_defs.iter().find(|f| f.name == r.field_name);
            let kind_label = field_def
                .map(|fd| match &fd.kind {
                    FieldKind::Bool => "bool",
                    FieldKind::Float => "float",
                    FieldKind::Integer => "int",
                    FieldKind::Text => "text",
                    FieldKind::Enum(_) => "enum",
                })
                .unwrap_or("?");
            fg(c, AnsiColor::White);
            bold(c, true);
            let t = format!(
                " {} ({})  type: {}",
                r.display_name, r.field_name, kind_label
            );
            bold(c, false);
            t
        }
        (Some(r), 1) => {
            let status_str = match r.status {
                FieldStatus::Inherited => "Inherited",
                FieldStatus::Editable => "Editable",
                FieldStatus::FixedByLua => "Fixed by Lua",
            };
            fg(c, AnsiColor::Silver);
            let val = r.proposed_value.as_ref().unwrap_or(&r.current_value);
            format!(" value: {}  status: {}", val, status_str)
        }
        (Some(r), 2) => {
            let field_def = state.field_defs.iter().find(|f| f.name == r.field_name);
            fg(c, AnsiColor::Grey);
            format!(" {}", field_def.map(|f| f.doc).unwrap_or(""))
        }
        _ => {
            fg(c, AnsiColor::Grey);
            String::new()
        }
    };
    c.push(Change::Text(pad_to(&text, w)));
    bold(c, false);
}

// ─── Footer ─────────────────────────────────────────────────────────────────

fn render_footer(c: &mut Vec<Change>, w: usize) {
    let hints: &[(&str, &str)] = &[
        ("↑↓", "Navigate"),
        ("Tab", "Sections"),
        ("Enter", "Edit"),
        ("/", "Search"),
        ("S", "Save"),
        ("P", "Preview"),
        ("R", "Reset"),
        ("Esc", "Close"),
    ];

    let mut text_len = 1; // leading space
    c.push(Change::Text(" ".into()));
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            fg(c, AnsiColor::Grey);
            c.push(Change::Text("  ".into()));
            text_len += 2;
        }
        fg(c, AnsiColor::White);
        bold(c, true);
        c.push(Change::Text((*key).into()));
        bold(c, false);
        fg(c, AnsiColor::Silver);
        c.push(Change::Text(format!(" {}", action)));
        text_len += key.len() + 1 + action.len();
    }
    let remaining = w.saturating_sub(text_len);
    c.push(Change::Text(" ".repeat(remaining)));
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn fg(c: &mut Vec<Change>, color: AnsiColor) {
    c.push(Change::Attribute(AttributeChange::Foreground(color.into())));
}

fn bg(c: &mut Vec<Change>, color: AnsiColor) {
    c.push(Change::Attribute(AttributeChange::Background(color.into())));
}

fn reset_bg(c: &mut Vec<Change>) {
    c.push(Change::Attribute(AttributeChange::Background(
        ColorAttribute::Default,
    )));
}

fn bold(c: &mut Vec<Change>, on: bool) {
    c.push(Change::Attribute(AttributeChange::Intensity(if on {
        Intensity::Bold
    } else {
        Intensity::Normal
    })));
}

fn reset(c: &mut Vec<Change>) {
    c.push(Change::Attribute(AttributeChange::Foreground(
        ColorAttribute::Default,
    )));
    c.push(Change::Attribute(AttributeChange::Background(
        ColorAttribute::Default,
    )));
    c.push(Change::Attribute(AttributeChange::Intensity(
        Intensity::Normal,
    )));
}

/// Truncate or pad a string to exactly `w` characters.
fn pad_to(s: &str, w: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= w {
        chars[..w].iter().collect()
    } else {
        let mut out: String = chars.into_iter().collect();
        out.extend(std::iter::repeat(' ').take(w - out.len()));
        out
    }
}

/// Truncate a string, appending "…" if needed.
fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max > 1 {
        let mut out: String = chars[..max - 1].iter().collect();
        out.push('…');
        out
    } else {
        "…".into()
    }
}
