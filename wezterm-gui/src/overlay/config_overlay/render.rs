//! Rendering helpers for the config overlay TUI.
//!
//! Builds a three-panel layout (sections / settings / details) using termwiz
//! `Change` sequences.
//!
//! --- weezterm remote features ---

use super::{FieldStatus, OverlayState, Panel, SettingRow};
use crate::overlay::config_overlay::data::FieldKind;
use termwiz::cell::{AttributeChange, Intensity};
use termwiz::color::AnsiColor;
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;

/// Panel column widths (characters).
const SECTION_PANEL_WIDTH: usize = 16;

/// Render the complete overlay frame.
pub fn render_frame(term: &mut impl Terminal, state: &OverlayState) -> anyhow::Result<()> {
    let screen = term.get_screen_size()?;
    let width = screen.cols.max(60);
    let height = screen.rows.max(10);

    let settings_panel_width = (width.saturating_sub(SECTION_PANEL_WIDTH + 2)) * 55 / 100;
    let details_panel_width = width.saturating_sub(SECTION_PANEL_WIDTH + settings_panel_width + 3);

    let mut changes = vec![
        Change::ClearScreen(Default::default()),
        Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(0),
        },
    ];

    // ── Header ───────────────────────────────────────────────────────
    render_header(&mut changes, state, width);

    // ── Separator ────────────────────────────────────────────────────
    push_separator(&mut changes, width);

    let body_height = height.saturating_sub(4); // header (2) + separator (1) + footer (1)

    // ── Body: three panels side by side ──────────────────────────────
    let settings = state.visible_settings();
    let selected_row = settings.get(state.selected_setting).cloned();

    for row_idx in 0..body_height.saturating_sub(1) {
        // Sections panel
        render_section_cell(&mut changes, state, row_idx, SECTION_PANEL_WIDTH);

        // Vertical separator
        push_attr(&mut changes, AnsiColor::Grey, false);
        changes.push(Change::Text("│".into()));

        // Settings panel
        render_setting_cell(
            &mut changes,
            state,
            &settings,
            row_idx,
            settings_panel_width,
        );

        // Vertical separator
        push_attr(&mut changes, AnsiColor::Grey, false);
        changes.push(Change::Text("│".into()));

        // Details panel
        render_detail_cell(
            &mut changes,
            &selected_row,
            state,
            row_idx,
            details_panel_width,
        );

        changes.push(Change::Text("\r\n".into()));
    }

    // ── Footer ───────────────────────────────────────────────────────
    push_separator(&mut changes, width);
    render_footer(&mut changes, state, width);

    term.render(&changes)?;
    term.flush()?;
    Ok(())
}

// ─── Header ─────────────────────────────────────────────────────────────────

fn render_header(changes: &mut Vec<Change>, state: &OverlayState, width: usize) {
    let section_name = state.current_section().display_name();
    let title = format!(" Configure WezTerm — {}", section_name);

    push_attr(changes, AnsiColor::White, true);
    changes.push(Change::Text(title.clone()));
    let pad = width.saturating_sub(title.len());
    changes.push(Change::Text(" ".repeat(pad)));
    changes.push(Change::Text("\r\n".into()));

    // Search bar
    push_attr(changes, AnsiColor::Grey, false);
    let search_label = " Search: ";
    changes.push(Change::Text(search_label.into()));

    if state.filter_active || !state.filter.is_empty() {
        push_attr(changes, AnsiColor::White, false);
        let display = if state.filter_active {
            format!("[ {}_ ]", state.filter)
        } else {
            format!("[ {} ]", state.filter)
        };
        changes.push(Change::Text(display));
    } else {
        push_attr(changes, AnsiColor::Grey, false);
        changes.push(Change::Text("[ type / to search ]".into()));
    }

    if let Some(ref edit) = state.inline_edit {
        let pad_before = 4;
        changes.push(Change::Text(" ".repeat(pad_before)));
        push_attr(changes, AnsiColor::Aqua, true);
        changes.push(Change::Text(format!("Edit: {}_", edit.buffer)));
    }

    if state.dirty {
        push_attr(changes, AnsiColor::Yellow, false);
        changes.push(Change::Text("  [modified]".into()));
    }

    changes.push(Change::Text("\r\n".into()));
}

// ─── Section panel (left) ───────────────────────────────────────────────────

fn render_section_cell(
    changes: &mut Vec<Change>,
    state: &OverlayState,
    row_idx: usize,
    col_width: usize,
) {
    if row_idx < state.sections.len() {
        let section = state.sections[row_idx];
        let is_selected = row_idx == state.selected_section;
        let is_focused = state.active_panel == Panel::Sections && is_selected;

        let prefix = if is_selected { " ▸ " } else { "   " };
        let label = section.display_name();
        let text = format!("{}{}", prefix, label);

        if is_focused {
            push_attr(changes, AnsiColor::Aqua, true);
        } else if is_selected {
            push_attr(changes, AnsiColor::White, true);
        } else {
            push_attr(changes, AnsiColor::Grey, false);
        }

        let truncated = truncate_or_pad(&text, col_width);
        changes.push(Change::Text(truncated));
    } else {
        changes.push(Change::Text(" ".repeat(col_width)));
    }
}

// ─── Settings panel (center) ────────────────────────────────────────────────

fn render_setting_cell(
    changes: &mut Vec<Change>,
    state: &OverlayState,
    settings: &[SettingRow],
    row_idx: usize,
    col_width: usize,
) {
    let actual_idx = row_idx + state.settings_scroll_offset;

    if actual_idx < settings.len() {
        let setting = &settings[actual_idx];
        let is_selected =
            actual_idx == state.selected_setting && state.active_panel == Panel::Settings;

        // Selection indicator
        let prefix = if is_selected { " ▸ " } else { "   " };

        // Field name
        let name_width = col_width.saturating_sub(18); // room for value + badge
        let name_text = truncate_or_pad(&setting.display_name, name_width);

        // Value display
        let value_text = setting
            .proposed_value
            .as_ref()
            .unwrap_or(&setting.current_value);
        let value_display = truncate_str(value_text, 10);

        // Badge
        let badge = match setting.status {
            FieldStatus::Inherited => "[I]",
            FieldStatus::Editable => "[E]",
            FieldStatus::FixedByLua => "[F]",
        };

        // Render
        if is_selected {
            push_attr(changes, AnsiColor::Aqua, true);
        } else {
            push_attr(changes, AnsiColor::White, false);
        }
        changes.push(Change::Text(prefix.into()));
        changes.push(Change::Text(name_text));

        // Value in different color
        if setting.proposed_value.is_some() {
            push_attr(changes, AnsiColor::Aqua, false);
        } else {
            push_attr(changes, AnsiColor::Grey, false);
        }
        changes.push(Change::Text(format!(" {}", value_display)));

        // Badge color
        match setting.status {
            FieldStatus::Inherited => push_attr(changes, AnsiColor::Grey, false),
            FieldStatus::Editable => push_attr(changes, AnsiColor::Green, false),
            FieldStatus::FixedByLua => push_attr(changes, AnsiColor::Yellow, false),
        }
        changes.push(Change::Text(format!(" {}", badge)));

        // Pad to fill column width
        let used = prefix.len() + name_width + 1 + value_display.len() + 1 + badge.len();
        if used < col_width {
            changes.push(Change::Text(" ".repeat(col_width - used)));
        }
    } else {
        changes.push(Change::Text(" ".repeat(col_width)));
    }
}

// ─── Details panel (right) ──────────────────────────────────────────────────

fn render_detail_cell(
    changes: &mut Vec<Change>,
    selected_row: &Option<SettingRow>,
    state: &OverlayState,
    row_idx: usize,
    col_width: usize,
) {
    let line = match selected_row {
        Some(row) => detail_line(row, state, row_idx),
        None => None,
    };

    if let Some((color, bold, text)) = line {
        push_attr(changes, color, bold);
        let truncated = truncate_or_pad(&format!(" {}", text), col_width);
        changes.push(Change::Text(truncated));
    } else {
        changes.push(Change::Text(" ".repeat(col_width)));
    }
}

fn detail_line(
    row: &SettingRow,
    state: &OverlayState,
    line_idx: usize,
) -> Option<(AnsiColor, bool, String)> {
    let field_def = state.field_defs.iter().find(|f| f.name == row.field_name)?;
    match line_idx {
        0 => Some((
            AnsiColor::White,
            true,
            format!("Setting: {}", row.display_name),
        )),
        1 => Some((AnsiColor::Grey, false, format!("Key: {}", row.field_name))),
        2 => Some((AnsiColor::Grey, false, String::new())),
        3 => Some((
            AnsiColor::White,
            false,
            format!("Effective: {}", row.current_value),
        )),
        4 => {
            if let Some(ref pv) = row.proposed_value {
                Some((AnsiColor::Aqua, false, format!("Proposed:  {}", pv)))
            } else {
                Some((AnsiColor::Grey, false, "Proposed:  —".into()))
            }
        }
        5 => Some((AnsiColor::Grey, false, String::new())),
        6 => {
            let (color, text) = match row.status {
                FieldStatus::Inherited => (AnsiColor::Grey, "Status: Inherited"),
                FieldStatus::Editable => (AnsiColor::Green, "Status: Editable"),
                FieldStatus::FixedByLua => (AnsiColor::Yellow, "Status: Fixed by Lua"),
            };
            Some((color, false, text.into()))
        }
        7 => {
            if row.status == FieldStatus::FixedByLua {
                Some((AnsiColor::Grey, false, "Lua config returned a".into()))
            } else {
                None
            }
        }
        8 => {
            if row.status == FieldStatus::FixedByLua {
                Some((AnsiColor::Grey, false, "different value.".into()))
            } else {
                None
            }
        }
        9 => Some((AnsiColor::Grey, false, String::new())),
        10 => Some((AnsiColor::Grey, false, field_def.doc.to_string())),
        12 => {
            let kind_str = match &field_def.kind {
                FieldKind::Bool => "Type: Boolean".into(),
                FieldKind::Float => "Type: Float".into(),
                FieldKind::Integer => "Type: Integer".into(),
                FieldKind::Text => "Type: Text".into(),
                FieldKind::Enum(v) => format!("Type: Enum ({})", v.join(", ")),
            };
            Some((AnsiColor::Grey, false, kind_str))
        }
        _ => None,
    }
}

// ─── Footer ─────────────────────────────────────────────────────────────────

fn render_footer(changes: &mut Vec<Change>, _state: &OverlayState, _width: usize) {
    push_attr(changes, AnsiColor::Grey, false);
    changes.push(Change::Text(" ".into()));

    let hints: &[(&str, &str)] = &[
        ("↑↓", "Navigate"),
        ("Enter", "Edit"),
        ("/", "Search"),
        ("P", "Preview"),
        ("R", "Reset"),
        ("S", "Save"),
        ("Esc", "Close"),
    ];

    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            push_attr(changes, AnsiColor::Grey, false);
            changes.push(Change::Text("  ".into()));
        }
        push_attr(changes, AnsiColor::White, true);
        changes.push(Change::Text((*key).into()));
        push_attr(changes, AnsiColor::Grey, false);
        changes.push(Change::Text(format!(" {}", action)));
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn push_attr(changes: &mut Vec<Change>, color: AnsiColor, bold: bool) {
    changes.push(Change::Attribute(AttributeChange::Foreground(color.into())));
    changes.push(Change::Attribute(AttributeChange::Intensity(if bold {
        Intensity::Bold
    } else {
        Intensity::Normal
    })));
}

fn push_separator(changes: &mut Vec<Change>, width: usize) {
    push_attr(changes, AnsiColor::Grey, false);
    let line = "─".repeat(width.min(200));
    changes.push(Change::Text(format!("{}\r\n", line)));
}

/// Truncate a string and pad with spaces to exactly `width` characters.
fn truncate_or_pad(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        chars[..width].iter().collect()
    } else {
        let mut out: String = chars.into_iter().collect();
        out.extend(std::iter::repeat(' ').take(width - out.len()));
        out
    }
}

/// Truncate a string to at most `max` characters, adding "…" if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max > 1 {
        let mut out: String = chars[..max - 1].iter().collect();
        out.push('…');
        out
    } else {
        "…".to_string()
    }
}
