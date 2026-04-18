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

/// Public wrapper for overlay geometry, used by mouse hit-testing.
pub fn overlay_rect_pub(width: u16, height: u16) -> (Rect, u16, u16) {
    overlay_rect(Rect::new(0, 0, width, height))
}

/// Main UI rendering function — called from `terminal.draw(|f| ui(f, ...))`.
pub fn ui(frame: &mut Frame, state: &mut OverlayState, theme: &Theme) -> LayoutGeo {
    let (area, left_pad, top_pad) = overlay_rect(frame.area());

    // Outer block with title
    let section_name = state.current_section().display_name();
    let title = format!(" Configure WeezTerm ── {} ", section_name);
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
    // Use more detail rows for the Monitors section to show the layout diagram
    let detail_rows = if state.current_section() == super::data::Section::Monitors {
        8
    } else {
        DETAIL_ROWS
    };
    let right_vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(detail_rows)])
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

    // ── Enum picker popup (rendered last so it draws on top) ────────
    if state.enum_picker.is_some() {
        render_enum_picker(frame, state, theme, area);
    }

    // ── Color scheme picker popup ────────────────────────────────────
    if state.scheme_picker.is_some() {
        render_scheme_picker(frame, state, theme, area);
    }

    // --- weezterm remote features ---
    // ── Monitor color scheme picker popup ─────────────────────────────
    if state.monitor_scheme_picker.is_some() {
        render_monitor_scheme_picker(frame, state, theme, area);
    }
    // --- end weezterm remote features ---

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

    // Fixed column widths: value (16 chars) + badge (10 chars)
    let value_w = 16u16;
    let badge_w = 10u16;
    let name_w = area.width.saturating_sub(value_w + badge_w + 3) as usize; // 3 for column spacing

    let rows: Vec<Row> = settings
        .iter()
        .enumerate()
        .map(|(i, setting)| {
            let is_selected = i == state.selected_setting && state.active_panel == Panel::Settings;
            let prefix = if is_selected { " \u{25b8} " } else { "   " };

            // Domain group header row
            if let Some(ref header) = setting.domain_header {
                let arrow = if header.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                let label = format!(" {} {} ", arrow, setting.display_name);
                let badge = match header.source {
                    super::data::DomainSource::Lua => "lua",
                    super::data::DomainSource::Overlay => "editable",
                };
                let (name_style, val_style, bdg_style) = if is_selected {
                    (theme.selected, theme.selected_value, theme.selected_badge)
                } else {
                    (
                        theme.text.add_modifier(Modifier::BOLD),
                        theme.value,
                        match header.source {
                            super::data::DomainSource::Lua => theme.badge_inherited,
                            super::data::DomainSource::Overlay => theme.badge_editable,
                        },
                    )
                };
                let sep_line = "\u{2500}".repeat(name_w.saturating_sub(label.len()).max(1));
                let name_cell = Line::from(vec![
                    Span::styled(label, name_style),
                    Span::styled(sep_line, theme.border),
                ]);
                return Row::new(vec![
                    ratatui::text::Text::from(name_cell),
                    ratatui::text::Text::styled(setting.current_value.clone(), val_style),
                    ratatui::text::Text::styled(badge.to_string(), bdg_style),
                ]);
            }

            // --- weezterm remote features ---
            // Monitor group header row
            if let Some(ref header) = setting.monitor_header {
                let arrow = if header.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                let current_marker = if header.is_current { " \u{25c6}" } else { "" };
                let label = format!(" {} {}{} ", arrow, setting.display_name, current_marker);
                let badge = if setting.status == FieldStatus::Inherited {
                    "inherited"
                } else {
                    "modified"
                };
                let (name_style, val_style, bdg_style) = if is_selected {
                    (theme.selected, theme.selected_value, theme.selected_badge)
                } else {
                    (
                        theme.text.add_modifier(Modifier::BOLD),
                        theme.value,
                        if setting.status == FieldStatus::Inherited {
                            theme.badge_inherited
                        } else {
                            theme.badge_editable
                        },
                    )
                };
                let sep_line = "\u{2500}".repeat(name_w.saturating_sub(label.len()).max(1));
                let name_cell = Line::from(vec![
                    Span::styled(label, name_style),
                    Span::styled(sep_line, theme.border),
                ]);
                return Row::new(vec![
                    ratatui::text::Text::from(name_cell),
                    ratatui::text::Text::styled(setting.current_value.clone(), val_style),
                    ratatui::text::Text::styled(badge.to_string(), bdg_style),
                ]);
            }
            // --- end weezterm remote features ---

            // "Add SSH Domain..." action row
            if setting.field_name == super::ADD_DOMAIN_FIELD_NAME {
                let label = format!("{}+ {}", prefix, setting.display_name);
                let style = if is_selected {
                    theme.selected
                } else {
                    theme.value_proposed
                };
                let name_cell = Line::from(Span::styled(label, style));
                return Row::new(vec![
                    ratatui::text::Text::from(name_cell),
                    ratatui::text::Text::raw(""),
                    ratatui::text::Text::raw(""),
                ]);
            }

            // "Delete Domain" action row
            if setting
                .field_name
                .starts_with(super::DELETE_DOMAIN_FIELD_NAME)
            {
                let label = format!("{}  \u{2717} Delete Domain", prefix);
                let style = if is_selected {
                    theme.selected
                } else {
                    theme.badge_fixed
                };
                let name_cell = Line::from(Span::styled(label, style));
                return Row::new(vec![
                    ratatui::text::Text::from(name_cell),
                    ratatui::text::Text::raw(""),
                    ratatui::text::Text::raw(""),
                ]);
            }

            // Regular setting row (including domain child fields)
            let name = &setting.display_name;
            let raw_value = setting
                .proposed_value
                .as_ref()
                .unwrap_or(&setting.current_value);
            // Display placeholder for empty values (rendering-only)
            let value = if raw_value.is_empty() {
                "\u{2014}" // em-dash as empty placeholder
            } else {
                raw_value.as_str()
            };
            let badge = match setting.status {
                FieldStatus::Inherited => "inherited",
                FieldStatus::Editable => "modified",
                FieldStatus::OverlayModified => "modified",
                FieldStatus::FixedByLua => "lua",
            };

            let label = format!("{}{}", prefix, name);
            let dots_len = name_w.saturating_sub(label.len() + 1).max(1);
            let dots = "\u{00b7}".repeat(dots_len); // middle dot

            let (name_style, dot_style, val_style, bdg_style) = if is_selected {
                (
                    theme.selected,
                    theme.dots.bg(theme.selection_bg),
                    theme.selected_value,
                    theme.selected_badge,
                )
            } else {
                let vs = if setting.proposed_value.is_some() {
                    theme.value_proposed
                } else {
                    theme.value
                };
                let bs = match setting.status {
                    FieldStatus::Inherited => theme.badge_inherited,
                    FieldStatus::Editable => theme.badge_editable,
                    FieldStatus::OverlayModified => theme.badge_editable,
                    FieldStatus::FixedByLua => theme.badge_fixed,
                };
                (theme.text, theme.dots, vs, bs)
            };

            // Name column: "  Name ········"
            let name_cell = Line::from(vec![
                Span::styled(label, name_style),
                Span::styled(" ", dot_style),
                Span::styled(dots, dot_style),
            ]);

            Row::new(vec![
                ratatui::text::Text::from(name_cell),
                ratatui::text::Text::styled(value.to_string(), val_style),
                ratatui::text::Text::styled(badge.to_string(), bdg_style),
            ])
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
    .column_spacing(1)
    .row_highlight_style(theme.selected);

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
            // Domain header: show domain summary
            if let Some(ref header) = row.domain_header {
                let source_str = match header.source {
                    super::data::DomainSource::Lua => "Lua config",
                    super::data::DomainSource::Overlay => "User-added",
                };
                vec![
                    Line::from(Span::styled(
                        format!(" Domain: {} ", row.display_name),
                        theme.detail_title,
                    )),
                    Line::from(Span::styled(
                        format!(" Host: {}  Source: {}", row.current_value, source_str),
                        theme.detail,
                    )),
                    Line::from(Span::styled(" Enter to expand/collapse", theme.text_dim)),
                ]
            } else if row.field_name == super::ADD_DOMAIN_FIELD_NAME {
                vec![
                    Line::from(Span::styled(" Add SSH Domain", theme.detail_title)),
                    Line::from(Span::styled(
                        " Press Enter to create a new SSH domain",
                        theme.detail,
                    )),
                ]
            } else if row.field_name.starts_with(super::DELETE_DOMAIN_FIELD_NAME) {
                vec![
                    Line::from(Span::styled(" Delete Domain", theme.detail_title)),
                    Line::from(Span::styled(
                        " Press Enter to remove this domain",
                        theme.detail,
                    )),
                ]
            // --- weezterm remote features ---
            } else if row.monitor_header.is_some() || row.monitor_child.is_some() {
                let selected_idx = row
                    .monitor_header
                    .as_ref()
                    .map(|h| h.monitor_index)
                    .or(row.monitor_child);
                render_monitor_layout_diagram(
                    &state.monitor_entries,
                    selected_idx,
                    area.width.saturating_sub(2) as usize,
                    area.height.saturating_sub(1) as usize,
                    theme,
                )
            // --- end weezterm remote features ---
            } else {
                let field_def = state.field_defs.iter().find(|f| f.name == row.field_name);
                let kind_label = field_def
                    .map(|fd| match &fd.kind {
                        FieldKind::Bool => "bool",
                        FieldKind::Float => "float",
                        FieldKind::Integer => "int",
                        FieldKind::Text => "text",
                        FieldKind::Enum(_) => "enum",
                        FieldKind::ColorScheme => "scheme",
                    })
                    .unwrap_or(match &row.kind {
                        FieldKind::Bool => "bool",
                        FieldKind::Float => "float",
                        FieldKind::Integer => "int",
                        FieldKind::Text => "text",
                        FieldKind::Enum(_) => "enum",
                        FieldKind::ColorScheme => "scheme",
                    });
                let status_str = match row.status {
                    FieldStatus::Inherited => "Inherited",
                    FieldStatus::Editable => "Editable",
                    FieldStatus::OverlayModified => "Modified",
                    FieldStatus::FixedByLua => "Fixed by Lua",
                };
                let val = row.proposed_value.as_ref().unwrap_or(&row.current_value);

                let doc = field_def.map(|f| f.doc).unwrap_or("");
                // For domain child fields, look up doc from domain_field_defs
                let domain_doc = if row.domain_child.is_some() {
                    super::data::domain_field_defs()
                        .iter()
                        .find(|(key, _, _, _)| row.field_name.ends_with(&format!("{}__", key)))
                        .map(|(_, _, _, d)| *d)
                        .unwrap_or(doc)
                } else {
                    doc
                };

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
                    Line::from(Span::styled(format!(" {}", domain_doc), theme.text_dim)),
                ]
            }
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

// --- weezterm remote features ---
// ─── Monitor layout diagram ────────────────────────────────────────────────

/// Render an ASCII layout diagram of monitor positions with numbered boxes.
/// Uses a connection-based approach so shared edges produce proper junctions
/// (┬ ┴ ├ ┤ ┼) instead of overwriting each other.
fn render_monitor_layout_diagram(
    monitors: &[super::data::MonitorOverrideEntry],
    selected_idx: Option<usize>,
    avail_w: usize,
    avail_h: usize,
    theme: &super::theme::Theme,
) -> Vec<Line<'static>> {
    use super::data::MonitorRect;

    let rects: Vec<(usize, MonitorRect)> = monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.screen_rect.map(|r| (i, r)))
        .collect();

    if rects.is_empty() {
        return vec![Line::from(Span::styled(
            " (no monitor geometry available)",
            theme.text_dim,
        ))];
    }

    // Bounding box
    let min_x = rects.iter().map(|(_, r)| r.x).min().unwrap();
    let min_y = rects.iter().map(|(_, r)| r.y).min().unwrap();
    let max_x = rects.iter().map(|(_, r)| r.x + r.width).max().unwrap();
    let max_y = rects.iter().map(|(_, r)| r.y + r.height).max().unwrap();

    let total_w = (max_x - min_x) as f64;
    let total_h = (max_y - min_y) as f64;
    if total_w <= 0.0 || total_h <= 0.0 {
        return vec![Line::from(Span::styled(
            " (invalid monitor geometry)",
            theme.text_dim,
        ))];
    }

    let draw_w = avail_w.saturating_sub(1).max(10);
    let draw_h = avail_h.max(3);

    // Scale to fit, preserving aspect ratio (terminal chars ~2:1)
    let char_aspect = 2.0_f64;
    let scale_x = draw_w as f64 / total_w;
    let scale_y = draw_h as f64 / (total_h / char_aspect);
    let scale = scale_x.min(scale_y);

    let grid_w = (total_w * scale).ceil() as usize + 1;
    let grid_h = (total_h / char_aspect * scale).ceil() as usize + 1;
    let grid_w = grid_w.min(draw_w).max(1);
    let grid_h = grid_h.min(draw_h).max(1);

    // Connection grid: at each point, which directions have border lines?
    // Packed as bits: 1=up, 2=down, 4=left, 8=right
    let mut conn: Vec<Vec<u8>> = vec![vec![0u8; grid_w]; grid_h];
    // Owner grid: which monitor index owns each cell (for styling)
    let mut owner: Vec<Vec<Option<usize>>> = vec![vec![None; grid_w]; grid_h];
    // Character overlay: for labels placed on top of interior cells
    let mut label_grid: Vec<Vec<Option<char>>> = vec![vec![None; grid_w]; grid_h];

    // Convert each monitor rect to grid coordinates and draw
    for &(idx, ref rect) in &rects {
        let x1 = ((rect.x - min_x) as f64 * scale).round() as usize;
        let y1 = ((rect.y - min_y) as f64 / char_aspect * scale).round() as usize;
        let x2 = (((rect.x + rect.width - min_x) as f64) * scale).round() as usize;
        let y2 =
            (((rect.y + rect.height - min_y) as f64) / char_aspect * scale).round() as usize;

        let x1 = x1.min(grid_w.saturating_sub(1));
        let x2 = x2.min(grid_w.saturating_sub(1)).max(x1 + 2);
        let y1 = y1.min(grid_h.saturating_sub(1));
        let y2 = y2.min(grid_h.saturating_sub(1)).max(y1 + 2);

        // Top border: horizontal connections
        for x in x1..x2 {
            if y1 < grid_h && x + 1 < grid_w {
                conn[y1][x] |= 8; // right
                conn[y1][x + 1] |= 4; // left
            }
        }
        // Bottom border
        for x in x1..x2 {
            if y2 < grid_h && x + 1 < grid_w {
                conn[y2][x] |= 8;
                conn[y2][x + 1] |= 4;
            }
        }
        // Left border: vertical connections
        for y in y1..y2 {
            if x1 < grid_w && y + 1 < grid_h {
                conn[y][x1] |= 2; // down
                conn[y + 1][x1] |= 1; // up
            }
        }
        // Right border
        for y in y1..y2 {
            if x2 < grid_w && y + 1 < grid_h {
                conn[y][x2] |= 2;
                conn[y + 1][x2] |= 1;
            }
        }

        // Fill interior ownership
        for y in y1..=y2.min(grid_h.saturating_sub(1)) {
            for x in x1..=x2.min(grid_w.saturating_sub(1)) {
                owner[y][x] = Some(idx);
            }
        }

        // Place label in center
        let center_y = (y1 + y2) / 2;
        let center_x = (x1 + x2) / 2;
        let is_current = monitors.get(idx).map_or(false, |m| m.is_current);
        let label = if is_current {
            format!("\u{25c6}{}", idx + 1)
        } else {
            format!("{}", idx + 1)
        };
        let label_start = center_x.saturating_sub(label.len() / 2);
        for (ci, ch) in label.chars().enumerate() {
            let px = label_start + ci;
            if center_y > y1 && center_y < y2 && px > x1 && px < x2 && px < grid_w {
                label_grid[center_y][px] = Some(ch);
            }
        }
    }

    // Resolve connections to box-drawing characters and build output
    let resolve = |c: u8| -> char {
        match (c & 1 != 0, c & 2 != 0, c & 4 != 0, c & 8 != 0) {
            // (up, down, left, right)
            (false, true, false, true) => '\u{250c}',  // ┌
            (false, true, true, false) => '\u{2510}',  // ┐
            (true, false, false, true) => '\u{2514}',  // └
            (true, false, true, false) => '\u{2518}',  // ┘
            (false, false, true, true) => '\u{2500}',  // ─
            (true, true, false, false) => '\u{2502}',  // │
            (true, true, false, true) => '\u{251c}',   // ├
            (true, true, true, false) => '\u{2524}',   // ┤
            (false, true, true, true) => '\u{252c}',   // ┬
            (true, false, true, true) => '\u{2534}',   // ┴
            (true, true, true, true) => '\u{253c}',    // ┼
            // Partial (dead-end lines)
            (false, true, false, false) => '\u{2502}', // │
            (true, false, false, false) => '\u{2502}', // │
            (false, false, true, false) => '\u{2500}', // ─
            (false, false, false, true) => '\u{2500}', // ─
            _ => ' ',
        }
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    for y in 0..grid_h {
        let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
        let mut current_text = String::new();
        let mut current_owner: Option<usize> = None;
        let mut current_is_border = false;

        for x in 0..grid_w {
            let c = conn[y][x];
            let own = owner[y][x];
            let lbl = label_grid[y][x];
            let is_border = c != 0;

            let ch = if let Some(label_ch) = lbl {
                label_ch
            } else if c != 0 {
                resolve(c)
            } else {
                ' '
            };

            // Flush span when owner or border-ness changes
            if own != current_owner || is_border != current_is_border {
                if !current_text.is_empty() {
                    let style = match current_owner {
                        Some(i) if Some(i) == selected_idx => theme.selected,
                        Some(_) if current_is_border => theme.border,
                        Some(_) => theme.text,
                        None => theme.text_dim,
                    };
                    spans.push(Span::styled(current_text, style));
                    current_text = String::new();
                }
                current_owner = own;
                current_is_border = is_border;
            }
            current_text.push(ch);
        }
        if !current_text.is_empty() {
            let style = match current_owner {
                Some(i) if Some(i) == selected_idx => theme.selected,
                Some(_) if current_is_border => theme.border,
                Some(_) => theme.text,
                None => theme.text_dim,
            };
            spans.push(Span::styled(current_text, style));
        }
        lines.push(Line::from(spans));
    }
    lines
}
// --- end weezterm remote features ---

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
        super::data::FieldKind::ColorScheme => "Type a color scheme name",
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
        let names: Vec<&str> = variants.iter().map(|(v, _)| v.as_str()).collect();
        lines.push(Line::from(Span::styled(
            format!(" Options: {}", names.join(", ")),
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

// ─── Enum picker popup ──────────────────────────────────────────────────────

fn render_enum_picker(frame: &mut Frame, state: &OverlayState, theme: &Theme, parent: Rect) {
    let picker = match &state.enum_picker {
        Some(p) => p,
        None => return,
    };

    let field_def = state
        .field_defs
        .iter()
        .find(|f| f.name == picker.field_name);
    let title = field_def
        .map(|f| f.display_name)
        .unwrap_or(picker.field_name.as_str());

    // Size the popup to fit the variants
    let max_variant_len = picker
        .variants
        .iter()
        .map(|(v, d)| v.len() + if d.is_empty() { 0 } else { d.len() + 3 })
        .max()
        .unwrap_or(20);
    let popup_w = (max_variant_len as u16 + 6)
        .min(parent.width.saturating_sub(4))
        .max(30);
    let popup_h = (picker.variants.len() as u16 + 4)
        .min(parent.height.saturating_sub(4))
        .max(5);
    let popup_x = parent.x + (parent.width.saturating_sub(popup_w)) / 2;
    let popup_y = parent.y + (parent.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

    frame.render_widget(ratatui::widgets::Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Span::styled(format!(" {} ", title), theme.header))
        .style(theme.text);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Compute max variant name width for alignment (prefix + name)
    let max_name_w = picker
        .variants
        .iter()
        .map(|(v, _)| v.len() + 4) // 4 = prefix " ▸ " or "   "
        .max()
        .unwrap_or(10);

    let items: Vec<ListItem> = picker
        .variants
        .iter()
        .enumerate()
        .map(|(i, (variant, desc))| {
            let is_sel = i == picker.selected;
            let prefix = if is_sel { " \u{25b8} " } else { "   " };
            let name_text = format!("{}{}", prefix, variant);
            // Pad variant name to align descriptions
            let padded = format!("{:<width$}", name_text, width = max_name_w);
            let mut spans = vec![Span::styled(
                padded,
                if is_sel { theme.selected } else { theme.text },
            )];
            if !desc.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", desc),
                    if is_sel {
                        theme.selected_badge
                    } else {
                        theme.text_dim
                    },
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    // Footer hint inside popup
    let mut all_items = items;
    if inner.height as usize > picker.variants.len() + 1 {
        all_items.push(ListItem::new(""));
        all_items.push(ListItem::new(Span::styled(
            " Enter select  Space cycle  Esc cancel",
            theme.text_dim,
        )));
    }

    let list = List::new(all_items);
    frame.render_widget(list, inner);
}

// ─── Color scheme picker popup ──────────────────────────────────────────────

fn render_scheme_picker(frame: &mut Frame, state: &OverlayState, _theme: &Theme, parent: Rect) {
    let picker = match &state.scheme_picker {
        Some(p) => p,
        None => return,
    };
    render_scheme_picker_common(frame, picker, _theme, parent, "Color Scheme");
}

fn render_scheme_picker_common(
    frame: &mut Frame,
    picker: &super::ColorSchemePicker,
    _theme: &Theme,
    parent: Rect,
    label: &str,
) {

    let popup_w = parent.width.saturating_sub(4).min(80).max(40);
    let popup_h = parent.height.saturating_sub(4).min(30).max(10);
    let popup_x = parent.x + (parent.width.saturating_sub(popup_w)) / 2;
    let popup_y = parent.y + (parent.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

    frame.render_widget(ratatui::widgets::Clear, popup_area);

    // Use selected scheme's colors for the popup border as a live preview
    let sel_palette = picker
        .filtered
        .get(picker.selected)
        .map(|&idx| &picker.schemes[idx].1);
    let preview_fg = sel_palette
        .and_then(|p| p.foreground)
        .map(|c| {
            let (r, g, b, _) = c.to_srgb_u8();
            ratatui::style::Color::Rgb(r, g, b)
        })
        .unwrap_or(ratatui::style::Color::White);
    let preview_bg = sel_palette
        .and_then(|p| p.background)
        .map(|c| {
            let (r, g, b, _) = c.to_srgb_u8();
            ratatui::style::Color::Rgb(r, g, b)
        })
        .unwrap_or(ratatui::style::Color::Black);

    let title = format!(
        " {} ({}/{}) ",
        label,
        picker.filtered.len(),
        picker.schemes.len()
    );

    let preview_style = ratatui::style::Style::default()
        .fg(preview_fg)
        .bg(preview_bg);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(preview_style)
        .title(Span::styled(title, preview_style))
        .style(preview_style);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(inner);

    let filter_bar = vert[0];
    let list_area = vert[1];

    let filter_display = if picker.filter.is_empty() {
        " / type to filter...".to_string()
    } else {
        format!(" / {}|", picker.filter)
    };
    let filter_para = Paragraph::new(Span::styled(filter_display, preview_style));
    frame.render_widget(filter_para, filter_bar);

    // Each scheme takes 2 lines
    let visible_items = list_area.height as usize / 2;
    let scroll = if picker.selected >= visible_items {
        picker.selected.saturating_sub(visible_items - 1)
    } else {
        0
    };

    let content_w = inner.width as usize;
    let swatch_w = 17usize;
    let name_col_w = content_w.saturating_sub(swatch_w + 1);

    let items: Vec<ListItem> = picker
        .filtered
        .iter()
        .skip(scroll)
        .take(visible_items)
        .enumerate()
        .map(|(vis_idx, &scheme_idx)| {
            let (name, palette) = &picker.schemes[scheme_idx];
            let is_sel = vis_idx + scroll == picker.selected;

            let scheme_fg = palette_color(palette.foreground, ratatui::style::Color::White);
            let scheme_bg = palette_color(palette.background, ratatui::style::Color::Black);

            let (row_fg, row_bg) = if is_sel {
                let sel_fg = palette_color(palette.selection_fg, scheme_bg);
                let sel_bg = palette_color(palette.selection_bg, scheme_fg);
                (sel_fg, sel_bg)
            } else {
                (scheme_fg, scheme_bg)
            };

            let base = ratatui::style::Style::default().fg(row_fg).bg(row_bg);

            let prefix = if is_sel { " \u{25b8} " } else { "   " };
            let display_name = format!("{}{}", prefix, name);
            let display_name = if display_name.len() > name_col_w {
                format!("{}...", &display_name[..name_col_w.saturating_sub(3)])
            } else {
                display_name
            };
            let pad_len = name_col_w.saturating_sub(display_name.len());
            let padding = " ".repeat(pad_len);

            let mut line1 = vec![
                Span::styled(display_name, base),
                Span::styled(padding, base),
            ];

            if let Some(ansi) = &palette.ansi {
                for color in ansi.iter() {
                    let c = rgba_to_color(color);
                    line1.push(Span::styled(
                        "\u{2588}",
                        ratatui::style::Style::default().fg(c).bg(row_bg),
                    ));
                }
            } else {
                line1.push(Span::styled("        ", base));
            }
            line1.push(Span::styled(" ", base));
            if let Some(brights) = &palette.brights {
                for color in brights.iter() {
                    let c = rgba_to_color(color);
                    line1.push(Span::styled(
                        "\u{2588}",
                        ratatui::style::Style::default().fg(c).bg(row_bg),
                    ));
                }
            } else {
                line1.push(Span::styled("        ", base));
            }

            let example = "   user@host:~ $ echo hello";
            let ex_len = example.len().min(content_w);
            let ex_pad = " ".repeat(content_w.saturating_sub(ex_len));
            let line2 = vec![
                Span::styled(&example[..ex_len], base),
                Span::styled(ex_pad, base),
            ];

            ListItem::new(vec![Line::from(line1), Line::from(line2)])
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, list_area);
}

fn palette_color(
    color: Option<config::RgbaColor>,
    fallback: ratatui::style::Color,
) -> ratatui::style::Color {
    color
        .map(|c| {
            let (r, g, b, _) = c.to_srgb_u8();
            ratatui::style::Color::Rgb(r, g, b)
        })
        .unwrap_or(fallback)
}

fn rgba_to_color(color: &config::RgbaColor) -> ratatui::style::Color {
    let (r, g, b, _) = color.to_srgb_u8();
    ratatui::style::Color::Rgb(r, g, b)
}

// --- weezterm remote features ---
fn render_monitor_scheme_picker(
    frame: &mut Frame,
    state: &OverlayState,
    theme: &Theme,
    parent: Rect,
) {
    let picker = match &state.monitor_scheme_picker {
        Some(p) => p,
        None => return,
    };

    let monitor_name = state
        .monitor_entries
        .get(state.selected_monitor)
        .map(|e| e.monitor_name.as_str())
        .unwrap_or("Monitor");

    render_scheme_picker_common(frame, picker, theme, parent, monitor_name);
}
// --- end weezterm remote features ---
