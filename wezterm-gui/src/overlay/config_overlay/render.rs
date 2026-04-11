//! Rendering the config overlay using Ratatui widgets.
//!
//! Uses Block, List, Table, Paragraph, and Layout to build a modern, themed
//! two-panel settings UI. All styles come from the `Theme` (derived from the
//! user's color palette).
//!
//! --- weezterm remote features ---

use super::{FieldStatus, OverlayState, Panel};
use crate::overlay::config_overlay::data::FieldKind;
use crate::overlay::config_overlay::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;

const MAX_WIDTH: u16 = 100;
const MAX_HEIGHT: u16 = 35;
const SECTION_W: u16 = 20;
const DETAIL_ROWS: u16 = 4;

/// Layout geometry for mouse hit-testing.
#[allow(dead_code)]
pub struct LayoutGeo {
    pub left_pad: u16,
    pub top_pad: u16,
    pub section_w: u16,
    pub body_area: Rect,
}

/// Compute centered overlay area, capped at MAX_WIDTH × MAX_HEIGHT but using
/// at least 90% of the terminal to avoid feeling too small.
fn overlay_rect(total: Rect) -> (Rect, u16, u16) {
    let w = total.width.min(MAX_WIDTH).max(total.width * 9 / 10);
    let w = w.min(total.width); // never exceed terminal
    let h = total.height.min(MAX_HEIGHT).max(total.height * 9 / 10);
    let h = h.min(total.height);
    let x = (total.width.saturating_sub(w)) / 2;
    let y = (total.height.saturating_sub(h)) / 2;
    (Rect::new(x, y, w, h), x, y)
}

/// Main UI rendering function — called from `terminal.draw(|f| ui(f, ...))`.
pub fn ui(frame: &mut Frame, state: &mut OverlayState, theme: &Theme) -> LayoutGeo {
    let (area, left_pad, top_pad) = overlay_rect(frame.area());

    // Outer block with title
    let section_name = state.current_section().display_name();
    let title = format!(" Configure WezTerm ── {} ", section_name);
    let mut title_spans = vec![Span::styled(title, theme.header)];
    if state.dirty {
        title_spans.push(Span::styled(" [modified] ", theme.badge_fixed));
    }
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Line::from(title_spans));

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    // Vertical split: search bar (1) + body + footer (1)
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search
            Constraint::Min(5),    // body
            Constraint::Length(1), // footer
        ])
        .split(inner);

    let search_area = vert[0];
    let body_area = vert[1];
    let footer_area = vert[2];

    // ── Search bar ───────────────────────────────────────────────────
    render_search(frame, state, theme, search_area);

    // ── Body: horizontal split into sections | settings ──────────────
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SECTION_W), Constraint::Min(30)])
        .split(body_area);

    let sections_area = horiz[0];
    let right_area = horiz[1];

    // Right area: split into settings + details
    let right_vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(DETAIL_ROWS)])
        .split(right_area);

    let settings_area = right_vert[0];
    let details_area = right_vert[1];

    // ── Sections panel ───────────────────────────────────────────────
    render_sections(frame, state, theme, sections_area);

    // ── Settings panel ───────────────────────────────────────────────
    render_settings(frame, state, theme, settings_area);

    // ── Details panel ────────────────────────────────────────────────
    render_details(frame, state, theme, details_area);

    // ── Footer ───────────────────────────────────────────────────────
    render_footer(frame, theme, footer_area);

    // ── Edit popup (rendered last so it draws on top) ────────────────
    if state.inline_edit.is_some() {
        render_edit_popup(frame, state, theme, area);
    }

    LayoutGeo {
        left_pad,
        top_pad,
        section_w: SECTION_W,
        body_area,
    }
}

// ─── Search bar ─────────────────────────────────────────────────────────────

fn render_search(frame: &mut Frame, state: &OverlayState, theme: &Theme, area: Rect) {
    let search_text = if state.filter_active || !state.filter.is_empty() {
        let cursor = if state.filter_active { "▏" } else { "" };
        format!(" / {}{}", state.filter, cursor)
    } else {
        " / search…".into()
    };

    let mut spans = vec![Span::styled(
        search_text,
        if state.filter_active {
            theme.text
        } else {
            theme.text_dim
        },
    )];

    if let Some(ref edit) = state.inline_edit {
        spans.push(Span::styled(
            format!("    Edit: {}▏", edit.buffer),
            theme.value_proposed,
        ));
    }

    let paragraph = Paragraph::new(Line::from(spans)).style(theme.header);
    frame.render_widget(paragraph, area);
}

// ─── Sections panel ─────────────────────────────────────────────────────────

fn render_sections(frame: &mut Frame, state: &mut OverlayState, theme: &Theme, area: Rect) {
    let items: Vec<ListItem> = state
        .sections
        .iter()
        .enumerate()
        .map(|(i, section)| {
            let is_current = i == state.selected_section;
            let prefix = if is_current { " ▸ " } else { "   " };
            let style = if is_current && state.active_panel == Panel::Sections {
                theme.section_active
            } else if is_current {
                theme.text.add_modifier(Modifier::BOLD)
            } else {
                theme.section_inactive
            };
            ListItem::new(format!("{}{}", prefix, section.display_name())).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme.border);

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

// ─── Settings panel ─────────────────────────────────────────────────────────

fn render_settings(frame: &mut Frame, state: &mut OverlayState, theme: &Theme, area: Rect) {
    let settings = state.visible_settings();
    let sel_bg = theme.selection_bg;

    // Fixed column widths: value (16 chars) + badge (10 chars)
    let value_w = 16u16;
    let badge_w = 10u16;

    let rows: Vec<Row> = settings
        .iter()
        .enumerate()
        .map(|(i, setting)| {
            let is_selected = i == state.selected_setting && state.active_panel == Panel::Settings;
            let prefix = if is_selected { " \u{25b8} " } else { "   " };
            let name = &setting.display_name;
            let value = setting
                .proposed_value
                .as_ref()
                .unwrap_or(&setting.current_value);
            let badge = match setting.status {
                FieldStatus::Inherited => "inherited",
                FieldStatus::Editable => "modified",
                FieldStatus::FixedByLua => "lua",
            };

            let name_text = format!("{}{}", prefix, name);

            let value_style = if setting.proposed_value.is_some() {
                theme.value_proposed
            } else {
                theme.value
            };
            let badge_style = match setting.status {
                FieldStatus::Inherited => theme.badge_inherited,
                FieldStatus::Editable => theme.badge_editable,
                FieldStatus::FixedByLua => theme.badge_fixed,
            };

            let name_style = if is_selected {
                theme.selected
            } else {
                theme.text
            };

            if is_selected {
                Row::new(vec![
                    ratatui::text::Text::styled(name_text, name_style),
                    ratatui::text::Text::styled(value.to_string(), value_style.bg(sel_bg)),
                    ratatui::text::Text::styled(badge.to_string(), badge_style.bg(sel_bg)),
                ])
                .style(theme.selected)
            } else {
                Row::new(vec![
                    ratatui::text::Text::styled(name_text, name_style),
                    ratatui::text::Text::styled(value.to_string(), value_style),
                    ratatui::text::Text::styled(badge.to_string(), badge_style),
                ])
            }
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(value_w),
            Constraint::Length(badge_w),
        ],
    )
    .column_spacing(1);

    let mut table_state = ratatui::widgets::TableState::default();
    table_state.select(Some(state.selected_setting));
    frame.render_stateful_widget(table, area, &mut table_state);
}

// ─── Details panel ──────────────────────────────────────────────────────────

fn render_details(frame: &mut Frame, state: &OverlayState, theme: &Theme, area: Rect) {
    let settings = state.visible_settings();
    let selected = settings.get(state.selected_setting);

    let lines = match selected {
        Some(row) => {
            let field_def = state.field_defs.iter().find(|f| f.name == row.field_name);
            let kind_label = field_def
                .map(|fd| match &fd.kind {
                    FieldKind::Bool => "bool",
                    FieldKind::Float => "float",
                    FieldKind::Integer => "int",
                    FieldKind::Text => "text",
                    FieldKind::Enum(_) => "enum",
                })
                .unwrap_or("?");
            let status_str = match row.status {
                FieldStatus::Inherited => "Inherited",
                FieldStatus::Editable => "Editable",
                FieldStatus::FixedByLua => "Fixed by Lua",
            };
            let val = row.proposed_value.as_ref().unwrap_or(&row.current_value);

            vec![
                Line::from(vec![
                    Span::styled(format!(" {} ", row.display_name), theme.detail_title),
                    Span::styled(format!("({})  ", row.field_name), theme.detail),
                    Span::styled(format!("type: {}", kind_label), theme.text_dim),
                ]),
                Line::from(vec![
                    Span::styled(format!(" value: {}  ", val), theme.value),
                    Span::styled(format!("status: {}", status_str), theme.detail),
                ]),
                Line::from(Span::styled(
                    format!(" {}", field_def.map(|f| f.doc).unwrap_or("")),
                    theme.text_dim,
                )),
            ]
        }
        None => vec![Line::from(Span::styled(
            " Select a setting to view details",
            theme.text_dim,
        ))],
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.border);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

// ─── Footer ─────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, theme: &Theme, area: Rect) {
    let hints = vec![
        ("↑↓", "Navigate"),
        ("Tab", "Switch pane"),
        ("Enter", "Edit"),
        ("/", "Search"),
        ("S", "Save"),
        ("P", "Preview"),
        ("R", "Reset"),
        ("Esc", "Close"),
    ];

    let mut spans = vec![Span::raw(" ")];
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme.footer));
        }
        spans.push(Span::styled(*key, theme.footer_key));
        spans.push(Span::styled(format!(" {}", action), theme.footer));
    }

    let paragraph = Paragraph::new(Line::from(spans)).style(theme.footer);
    frame.render_widget(paragraph, area);
}

// ─── Edit popup (centered over settings area) ───────────────────────────────

fn render_edit_popup(frame: &mut Frame, state: &OverlayState, theme: &Theme, parent: Rect) {
    let edit = match &state.inline_edit {
        Some(e) => e,
        None => return,
    };

    let field_def = state.field_defs.iter().find(|f| f.name == edit.field_name);
    let title = field_def
        .map(|f| f.display_name)
        .unwrap_or(edit.field_name.as_str());

    let popup_w = 44.min(parent.width.saturating_sub(4));
    let popup_h = 7.min(parent.height.saturating_sub(4));
    let popup_x = parent.x + (parent.width.saturating_sub(popup_w)) / 2;
    let popup_y = parent.y + (parent.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

    // Clear the popup area
    frame.render_widget(ratatui::widgets::Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Span::styled(format!(" Edit: {} ", title), theme.header))
        .style(theme.text);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let kind_hint = match &edit.kind {
        super::data::FieldKind::Bool => "Toggle with Enter or Space",
        super::data::FieldKind::Float => "Enter a number (e.g. 14.0)",
        super::data::FieldKind::Integer => "Enter a whole number",
        super::data::FieldKind::Text => "Type a value",
        super::data::FieldKind::Enum(_) => "Choose from options below",
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {}|", edit.buffer),
            theme.value_proposed,
        )),
        Line::from(""),
        Line::from(Span::styled(format!(" {}", kind_hint), theme.text_dim)),
    ];

    if let super::data::FieldKind::Enum(variants) = &edit.kind {
        lines.push(Line::from(Span::styled(
            format!(" Options: {}", variants.join(", ")),
            theme.text_dim,
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " Enter to confirm, Esc to cancel",
            theme.text_dim,
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
