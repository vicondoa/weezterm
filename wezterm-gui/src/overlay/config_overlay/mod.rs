//! Config overlay — a three-panel TUI for browsing and editing WezTerm settings.
//!
//! Opens via the command palette or `ShowConfigOverlay` key assignment.
//! Proposes config values; Lua remains the source of truth.
//!
//! --- weezterm remote features ---

use ratatui::backend::Backend as _;
use std::collections::HashMap;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::terminal::Terminal;
use wezterm_dynamic::Value;

pub mod backend;
pub mod data;
pub mod persistence;
mod render;
pub mod theme;

pub use data::{FieldDef, FieldKind, Section, SshDomainConfig};

/// Result returned by the config overlay to the caller.
#[derive(Debug, Clone)]
pub enum ConfigOverlayAction {
    /// User closed the overlay without saving.
    Close,
    /// User chose to save proposals (includes domain changes).
    Save {
        proposals: HashMap<String, Value>,
        ssh_domains: Vec<SshDomainConfig>,
    },
    /// User chose to preview proposals (apply as window overrides).
    Preview(HashMap<String, Value>),
}

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Sections,
    Settings,
}

/// Status of a config field relative to the overlay proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldStatus {
    /// No proposal — using the default/Lua value.
    Inherited,
    /// Proposal matches effective value.
    Editable,
    /// Proposal differs from effective value — Lua overrode it.
    FixedByLua,
}

/// A displayable setting row.
#[derive(Debug, Clone)]
pub(crate) struct SettingRow {
    pub field_name: String,
    pub display_name: String,
    pub current_value: String,
    pub proposed_value: Option<String>,
    pub status: FieldStatus,
    pub kind: FieldKind,
    /// If this is a domain group header, holds the domain index.
    pub domain_header: Option<DomainHeaderInfo>,
    /// If this is a domain child field, holds the domain index.
    pub domain_child: Option<usize>,
}

/// Info for a domain group header row.
#[derive(Debug, Clone)]
pub(crate) struct DomainHeaderInfo {
    pub domain_index: usize,
    pub source: data::DomainSource,
    pub expanded: bool,
}

/// Sentinel row type for "Add SSH Domain..." action.
const ADD_DOMAIN_FIELD_NAME: &str = "__add_ssh_domain__";
/// Sentinel row type for "Delete Domain" action.
const DELETE_DOMAIN_FIELD_NAME: &str = "__delete_domain__";

/// Internal state for the overlay.
struct OverlayState {
    active_panel: Panel,
    sections: Vec<Section>,
    selected_section: usize,
    selected_setting: usize,
    settings_scroll_offset: usize,
    filter: String,
    filter_active: bool,
    proposals: HashMap<String, Value>,
    effective_values: HashMap<String, Value>,
    #[allow(dead_code)]
    default_values: HashMap<String, Value>,
    field_defs: Vec<FieldDef>,
    dirty: bool,
    /// Inline edit mode: field name + buffer
    inline_edit: Option<InlineEdit>,
    /// Enum picker popup: field name + variants + selected index
    enum_picker: Option<EnumPicker>,
    /// Color scheme picker popup.
    scheme_picker: Option<ColorSchemePicker>,
    /// Domain entries (Lua-sourced + overlay-sourced).
    domain_entries: Vec<data::DomainEntry>,
    /// Domain being added/edited inline (None = not in domain edit mode).
    adding_domain: Option<SshDomainConfig>,
    /// Whether domain_entries has been modified.
    domains_dirty: bool,
}

/// State for inline editing of a field value.
struct InlineEdit {
    field_name: String,
    buffer: String,
    kind: FieldKind,
}

/// State for the enum selection popup.
struct EnumPicker {
    field_name: String,
    variants: Vec<(String, String)>,
    selected: usize,
}

/// State for the color scheme picker popup.
pub(crate) struct ColorSchemePicker {
    pub schemes: Vec<(String, config::Palette)>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
}

impl ColorSchemePicker {
    fn refilter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered = if filter_lower.is_empty() {
            (0..self.schemes.len()).collect()
        } else {
            self.schemes
                .iter()
                .enumerate()
                .filter(|(_, (name, _))| name.to_lowercase().contains(&filter_lower))
                .map(|(i, _)| i)
                .collect()
        };
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

impl OverlayState {
    fn new(
        effective_values: HashMap<String, Value>,
        default_values: HashMap<String, Value>,
        saved_proposals: HashMap<String, Value>,
        saved_domains: Vec<SshDomainConfig>,
    ) -> Self {
        let mut field_defs = data::get_field_defs();
        let sections = data::get_sections();

        // Enrich field docs from ConfigMeta (/// doc comments on Config fields)
        let config = config::configuration();
        data::enrich_docs_from_config(&mut field_defs, &config);

        let mut proposals = HashMap::new();
        for (k, v) in saved_proposals {
            proposals.insert(k, v);
        }

        // Build domain entries: Lua-sourced (read-only) + overlay-sourced (editable)
        let mut domain_entries = data::domains_from_config();
        for saved_dom in saved_domains {
            // Skip overlay domains that duplicate a Lua domain name
            if domain_entries
                .iter()
                .any(|e| e.config.name == saved_dom.name)
            {
                continue;
            }
            domain_entries.push(data::DomainEntry {
                config: saved_dom,
                source: data::DomainSource::Overlay,
                expanded: false,
            });
        }

        Self {
            active_panel: Panel::Settings,
            sections,
            selected_section: 0,
            selected_setting: 0,
            settings_scroll_offset: 0,
            filter: String::new(),
            filter_active: false,
            proposals,
            effective_values,
            default_values,
            field_defs,
            dirty: false,
            inline_edit: None,
            enum_picker: None,
            scheme_picker: None,
            domain_entries,
            adding_domain: None,
            domains_dirty: false,
        }
    }

    fn current_section(&self) -> Section {
        self.sections[self.selected_section]
    }

    /// Get the setting rows for the currently selected section, filtered.
    fn visible_settings(&self) -> Vec<SettingRow> {
        let section = self.current_section();
        let filter_lower = self.filter.to_lowercase();

        let mut rows: Vec<SettingRow> = self
            .field_defs
            .iter()
            .filter(|f| f.section == section)
            .filter(|f| {
                if filter_lower.is_empty() {
                    return true;
                }
                f.display_name.to_lowercase().contains(&filter_lower)
                    || f.name.to_lowercase().contains(&filter_lower)
            })
            .map(|f| {
                let effective_val = self.effective_values.get(f.name);
                let proposed_val = self.proposals.get(f.name);

                let current_value = effective_val
                    .map(|v| data::value_to_display_string(v))
                    .unwrap_or_else(|| "-".to_string());

                let proposed_value = proposed_val.map(|v| data::value_to_display_string(v));

                let status = match proposed_val {
                    None => FieldStatus::Inherited,
                    Some(pv) => match effective_val {
                        Some(ev) if data::values_equal(pv, ev) => FieldStatus::Editable,
                        _ => FieldStatus::FixedByLua,
                    },
                };

                SettingRow {
                    field_name: f.name.to_string(),
                    display_name: f.display_name.to_string(),
                    current_value,
                    proposed_value,
                    status,
                    kind: f.kind.clone(),
                    domain_header: None,
                    domain_child: None,
                }
            })
            .collect();

        // For SSH & Domains section, append domain group rows
        if section == Section::SshAndDomains {
            let domain_fields = data::domain_field_defs();
            let filter_matches_domain = |entry: &data::DomainEntry| -> bool {
                if filter_lower.is_empty() {
                    return true;
                }
                entry.config.name.to_lowercase().contains(&filter_lower)
                    || entry
                        .config
                        .remote_address
                        .to_lowercase()
                        .contains(&filter_lower)
            };

            for (idx, entry) in self.domain_entries.iter().enumerate() {
                if !filter_matches_domain(entry) {
                    continue;
                }

                // Domain group header
                rows.push(SettingRow {
                    field_name: format!("__domain_header_{}__", idx),
                    display_name: entry.config.name.clone(),
                    current_value: entry.config.remote_address.clone(),
                    proposed_value: None,
                    status: match entry.source {
                        data::DomainSource::Lua => FieldStatus::Inherited,
                        data::DomainSource::Overlay => FieldStatus::Editable,
                    },
                    kind: FieldKind::Text,
                    domain_header: Some(DomainHeaderInfo {
                        domain_index: idx,
                        source: entry.source,
                        expanded: entry.expanded,
                    }),
                    domain_child: None,
                });

                // If expanded, show child fields
                if entry.expanded {
                    for (field_key, field_display, field_kind, _doc) in &domain_fields {
                        let value = data::domain_field_value(&entry.config, field_key);
                        let is_editable = entry.source == data::DomainSource::Overlay;
                        rows.push(SettingRow {
                            field_name: format!("__domain_{}_{}__", idx, field_key),
                            display_name: format!("  {}", field_display),
                            current_value: value,
                            proposed_value: None,
                            status: if is_editable {
                                FieldStatus::Editable
                            } else {
                                FieldStatus::Inherited
                            },
                            kind: field_kind.clone(),
                            domain_header: None,
                            domain_child: Some(idx),
                        });
                    }

                    // Delete action for overlay domains
                    if entry.source == data::DomainSource::Overlay {
                        rows.push(SettingRow {
                            field_name: format!("{}_{}", DELETE_DOMAIN_FIELD_NAME, idx),
                            display_name: "  Delete Domain".to_string(),
                            current_value: String::new(),
                            proposed_value: None,
                            status: FieldStatus::Editable,
                            kind: FieldKind::Text,
                            domain_header: None,
                            domain_child: Some(idx),
                        });
                    }
                }
            }

            // "Add SSH Domain..." action row
            if filter_lower.is_empty() || "add ssh domain".contains(&filter_lower) {
                rows.push(SettingRow {
                    field_name: ADD_DOMAIN_FIELD_NAME.to_string(),
                    display_name: "Add SSH Domain...".to_string(),
                    current_value: String::new(),
                    proposed_value: None,
                    status: FieldStatus::Editable,
                    kind: FieldKind::Text,
                    domain_header: None,
                    domain_child: None,
                });
            }
        }

        rows
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_settings().len();
        if count == 0 {
            self.selected_setting = 0;
        } else if self.selected_setting >= count {
            self.selected_setting = count.saturating_sub(1);
        }
    }

    fn selected_row(&self) -> Option<SettingRow> {
        let settings = self.visible_settings();
        settings.into_iter().nth(self.selected_setting)
    }

    /// Returns true if the row can be edited (not FixedByLua).
    fn is_row_editable(row: &SettingRow) -> bool {
        row.status != FieldStatus::FixedByLua
    }

    /// Open the color scheme picker popup.
    fn open_scheme_picker(&mut self) {
        let schemes = data::get_color_schemes();
        let current = self
            .proposals
            .get("color_scheme")
            .or_else(|| self.effective_values.get("color_scheme"))
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let filtered: Vec<usize> = (0..schemes.len()).collect();
        let selected = schemes
            .iter()
            .position(|(name, _)| name == &current)
            .unwrap_or(0);

        self.scheme_picker = Some(ColorSchemePicker {
            schemes,
            filtered,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        });
    }

    /// Apply an edit: toggle bool, cycle enum, or accept inline text.
    fn apply_edit_for_field(&mut self, field_name: &str, new_value: Value) {
        self.proposals.insert(field_name.to_string(), new_value);
        self.dirty = true;
    }

    fn remove_proposal(&mut self, field_name: &str) {
        self.proposals.remove(field_name);
        self.dirty = true;
    }

    fn toggle_bool(&mut self, field_name: &str) {
        let current = self
            .proposals
            .get(field_name)
            .or_else(|| self.effective_values.get(field_name));
        let new_val = match current {
            Some(Value::Bool(b)) => Value::Bool(!b),
            _ => Value::Bool(true),
        };
        self.apply_edit_for_field(field_name, new_val);
    }

    fn cycle_enum(&mut self, field_name: &str, direction: isize) {
        let field_def = match self.field_defs.iter().find(|f| f.name == field_name) {
            Some(f) => f,
            None => return,
        };
        let variants = match &field_def.kind {
            FieldKind::Enum(variants) => variants,
            _ => return,
        };
        if variants.is_empty() {
            return;
        }

        let current_str = self
            .proposals
            .get(field_name)
            .or_else(|| self.effective_values.get(field_name))
            .map(|v| data::value_to_display_string(v));

        let current_idx = current_str
            .as_ref()
            .and_then(|s| variants.iter().position(|(v, _)| v == s))
            .unwrap_or(0);

        let new_idx = if direction > 0 {
            (current_idx + 1) % variants.len()
        } else {
            (current_idx + variants.len() - 1) % variants.len()
        };

        self.apply_edit_for_field(field_name, Value::String(variants[new_idx].0.clone()));
    }

    /// Handle Enter on a domain-related row.
    fn handle_domain_enter(&mut self, row: &SettingRow) {
        // Domain header: toggle expand/collapse
        if let Some(ref header) = row.domain_header {
            self.domain_entries[header.domain_index].expanded =
                !self.domain_entries[header.domain_index].expanded;
            return;
        }

        // "Add SSH Domain..." action
        if row.field_name == ADD_DOMAIN_FIELD_NAME {
            self.adding_domain = Some(SshDomainConfig::default());
            // Open inline edit for name
            self.inline_edit = Some(InlineEdit {
                field_name: "__new_domain_name__".to_string(),
                buffer: String::new(),
                kind: FieldKind::Text,
            });
            return;
        }

        // "Delete Domain" action
        if row.field_name.starts_with(DELETE_DOMAIN_FIELD_NAME) {
            if let Some(domain_idx) = row.domain_child {
                if domain_idx < self.domain_entries.len()
                    && self.domain_entries[domain_idx].source == data::DomainSource::Overlay
                {
                    self.domain_entries.remove(domain_idx);
                    self.domains_dirty = true;
                    self.dirty = true;
                }
            }
            return;
        }

        // Domain child field (editable): open editor
        if let Some(domain_idx) = row.domain_child {
            if domain_idx < self.domain_entries.len()
                && self.domain_entries[domain_idx].source == data::DomainSource::Overlay
            {
                // Extract the actual field key from __domain_N_fieldkey__
                if let Some(field_key) = extract_domain_field_key(&row.field_name) {
                    match &row.kind {
                        FieldKind::Bool => {
                            self.toggle_domain_bool(domain_idx, &field_key);
                        }
                        FieldKind::Enum(variants) => {
                            self.enum_picker = Some(EnumPicker {
                                field_name: row.field_name.clone(),
                                variants: variants.clone(),
                                selected: variants
                                    .iter()
                                    .position(|(v, _)| v == &row.current_value)
                                    .unwrap_or(0),
                            });
                        }
                        FieldKind::Float | FieldKind::Integer | FieldKind::Text => {
                            self.inline_edit = Some(InlineEdit {
                                field_name: row.field_name.clone(),
                                buffer: row.current_value.clone(),
                                kind: row.kind.clone(),
                            });
                        }
                        FieldKind::ColorScheme => {
                            // Domain fields don't use color scheme picker
                        }
                    }
                }
            }
        }
    }

    fn toggle_domain_bool(&mut self, domain_idx: usize, field_key: &str) {
        if let Some(entry) = self.domain_entries.get_mut(domain_idx) {
            let current = data::domain_field_value(&entry.config, field_key);
            let new_val = if current == "On" { "Off" } else { "On" };
            data::set_domain_field(&mut entry.config, field_key, new_val);
            self.domains_dirty = true;
            self.dirty = true;
        }
    }

    fn apply_domain_field_edit(&mut self, field_name: &str, value: &str) {
        // Parse __domain_N_fieldkey__ format
        if let Some((domain_idx, field_key)) = parse_domain_field_name(field_name) {
            if let Some(entry) = self.domain_entries.get_mut(domain_idx) {
                if entry.source == data::DomainSource::Overlay {
                    data::set_domain_field(&mut entry.config, &field_key, value);
                    self.domains_dirty = true;
                    self.dirty = true;
                }
            }
        }
    }

    /// Finalize adding a new domain.
    fn finalize_add_domain(&mut self, name: String) {
        if name.is_empty() {
            self.adding_domain = None;
            return;
        }
        // Check for name uniqueness
        if self.domain_entries.iter().any(|e| e.config.name == name) {
            // Name already exists, discard
            self.adding_domain = None;
            return;
        }
        let mut config = self.adding_domain.take().unwrap_or_default();
        config.name = name;
        self.domain_entries.push(data::DomainEntry {
            config,
            source: data::DomainSource::Overlay,
            expanded: true,
        });
        self.domains_dirty = true;
        self.dirty = true;
    }

    /// Get overlay-managed domains for saving.
    fn overlay_domains(&self) -> Vec<SshDomainConfig> {
        self.domain_entries
            .iter()
            .filter(|e| e.source == data::DomainSource::Overlay)
            .map(|e| e.config.clone())
            .collect()
    }
}

/// Extract the field key from a domain child field name like `__domain_2_remote_address__`.
fn extract_domain_field_key(field_name: &str) -> Option<String> {
    // Format: __domain_N_fieldkey__
    let trimmed = field_name
        .trim_start_matches("__domain_")
        .trim_end_matches("__");
    // Skip the index part (everything before the first '_')
    if let Some(pos) = trimmed.find('_') {
        Some(trimmed[pos + 1..].to_string())
    } else {
        None
    }
}

/// Parse a domain field name into (domain_index, field_key).
fn parse_domain_field_name(field_name: &str) -> Option<(usize, String)> {
    let trimmed = field_name
        .trim_start_matches("__domain_")
        .trim_end_matches("__");
    if let Some(pos) = trimmed.find('_') {
        let idx_str = &trimmed[..pos];
        let field_key = &trimmed[pos + 1..];
        if let Ok(idx) = idx_str.parse::<usize>() {
            return Some((idx, field_key.to_string()));
        }
    }
    None
}

/// Main entry point for the config overlay.
///
/// Called from `start_overlay()` in a background thread.
pub fn run_config_overlay(
    mut term: impl Terminal,
    effective_values: HashMap<String, Value>,
    default_values: HashMap<String, Value>,
    saved_proposals: HashMap<String, Value>,
    saved_domains: Vec<SshDomainConfig>,
    palette: config::Palette,
) -> anyhow::Result<ConfigOverlayAction> {
    let mut state = OverlayState::new(
        effective_values,
        default_values,
        saved_proposals,
        saved_domains,
    );
    let theme = theme::Theme::from_palette(&palette);

    term.set_raw_mode()?;
    let mut ratatui_backend = backend::TermwizOverlayBackend::new(term)?;
    ratatui_backend.hide_cursor()?;
    let mut ratatui_term = ratatui::Terminal::new(ratatui_backend)?;

    loop {
        state.clamp_selection();
        let _layout_geo = ratatui_term.draw(|frame| {
            render::ui(frame, &mut state, &theme);
        })?;

        let input = ratatui_term.backend_mut().terminal_mut().poll_input(None);

        match input {
            Ok(Some(input)) => {
                // If in inline edit mode, handle separately
                if let Some(ref mut edit) = state.inline_edit {
                    match input {
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Escape,
                            ..
                        }) => {
                            state.inline_edit = None;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Enter,
                            ..
                        }) => {
                            let field_name = edit.field_name.clone();
                            let buffer = edit.buffer.clone();
                            let kind = edit.kind.clone();
                            state.inline_edit = None;

                            // Handle new domain name entry
                            if field_name == "__new_domain_name__" {
                                state.finalize_add_domain(buffer);
                                continue;
                            }

                            // Handle domain child field edits
                            if field_name.starts_with("__domain_") {
                                state.apply_domain_field_edit(&field_name, &buffer);
                                continue;
                            }

                            let value = match kind {
                                FieldKind::Float => {
                                    if let Ok(f) = buffer.parse::<f64>() {
                                        Some(Value::F64(f.into()))
                                    } else {
                                        None
                                    }
                                }
                                FieldKind::Integer => {
                                    if let Ok(i) = buffer.parse::<i64>() {
                                        Some(Value::I64(i))
                                    } else {
                                        None
                                    }
                                }
                                FieldKind::Text => Some(Value::String(buffer)),
                                _ => None,
                            };
                            if let Some(v) = value {
                                state.apply_edit_for_field(&field_name, v);
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Backspace,
                            ..
                        }) => {
                            edit.buffer.pop();
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Char(c),
                            ..
                        }) => {
                            edit.buffer.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // If in enum picker mode, handle picker input
                if let Some(ref mut picker) = state.enum_picker {
                    match input {
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Escape,
                            ..
                        }) => {
                            state.enum_picker = None;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Enter,
                            ..
                        }) => {
                            let field_name = picker.field_name.clone();
                            let variant = picker.variants[picker.selected].0.clone();
                            state.enum_picker = None;
                            if field_name.starts_with("__domain_") {
                                state.apply_domain_field_edit(&field_name, &variant);
                            } else {
                                state.apply_edit_for_field(&field_name, Value::String(variant));
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::UpArrow,
                            ..
                        })
                        | InputEvent::Key(KeyEvent {
                            key: KeyCode::Char('k'),
                            modifiers: Modifiers::NONE,
                        }) => {
                            if picker.selected > 0 {
                                picker.selected -= 1;
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::DownArrow,
                            ..
                        })
                        | InputEvent::Key(KeyEvent {
                            key: KeyCode::Char('j'),
                            modifiers: Modifiers::NONE,
                        }) => {
                            if picker.selected + 1 < picker.variants.len() {
                                picker.selected += 1;
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Char(' '),
                            ..
                        }) => {
                            // Space in picker also selects
                            let field_name = picker.field_name.clone();
                            let variant = picker.variants[picker.selected].0.clone();
                            state.enum_picker = None;
                            if field_name.starts_with("__domain_") {
                                state.apply_domain_field_edit(&field_name, &variant);
                            } else {
                                state.apply_edit_for_field(&field_name, Value::String(variant));
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // If in color scheme picker mode, handle picker input
                if let Some(ref mut picker) = state.scheme_picker {
                    match input {
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Escape,
                            ..
                        }) => {
                            state.scheme_picker = None;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Enter,
                            ..
                        }) => {
                            if let Some(&idx) = picker.filtered.get(picker.selected) {
                                let name = picker.schemes[idx].0.clone();
                                state.scheme_picker = None;
                                state.apply_edit_for_field("color_scheme", Value::String(name));
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::UpArrow,
                            ..
                        })
                        | InputEvent::Key(KeyEvent {
                            key: KeyCode::Char('k'),
                            modifiers: Modifiers::CTRL,
                        }) => {
                            if picker.selected > 0 {
                                picker.selected -= 1;
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::DownArrow,
                            ..
                        })
                        | InputEvent::Key(KeyEvent {
                            key: KeyCode::Char('j'),
                            modifiers: Modifiers::CTRL,
                        }) => {
                            if picker.selected + 1 < picker.filtered.len() {
                                picker.selected += 1;
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Backspace,
                            ..
                        }) => {
                            picker.filter.pop();
                            picker.refilter();
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Char(c),
                            ..
                        }) => {
                            picker.filter.push(c);
                            picker.refilter();
                        }
                        _ => {}
                    }
                    continue;
                }

                // If in filter mode, handle filter input
                if state.filter_active {
                    match input {
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Escape,
                            ..
                        }) => {
                            if state.filter.is_empty() {
                                state.filter_active = false;
                            } else {
                                state.filter.clear();
                                state.selected_setting = 0;
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Enter,
                            ..
                        }) => {
                            state.filter_active = false;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Backspace,
                            ..
                        }) => {
                            state.filter.pop();
                            state.selected_setting = 0;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Char(c),
                            ..
                        }) => {
                            state.filter.push(c);
                            state.selected_setting = 0;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Normal mode input handling
                match input {
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Escape,
                        ..
                    }) => {
                        return Ok(ConfigOverlayAction::Close);
                    }

                    // Search
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('/'),
                        ..
                    }) => {
                        state.filter_active = true;
                        state.filter.clear();
                    }

                    // Navigate up
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::UpArrow,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('k'),
                        modifiers: Modifiers::NONE,
                    }) => match state.active_panel {
                        Panel::Sections => {
                            if state.selected_section > 0 {
                                state.selected_section -= 1;
                                state.selected_setting = 0;
                                state.settings_scroll_offset = 0;
                            }
                        }
                        Panel::Settings => {
                            if state.selected_setting > 0 {
                                state.selected_setting -= 1;
                            }
                        }
                    },

                    // Navigate down
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::DownArrow,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('j'),
                        modifiers: Modifiers::NONE,
                    }) => match state.active_panel {
                        Panel::Sections => {
                            if state.selected_section + 1 < state.sections.len() {
                                state.selected_section += 1;
                                state.selected_setting = 0;
                                state.settings_scroll_offset = 0;
                            }
                        }
                        Panel::Settings => {
                            let count = state.visible_settings().len();
                            if state.selected_setting + 1 < count {
                                state.selected_setting += 1;
                            }
                        }
                    },

                    // Tab to switch panels
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Tab, ..
                    }) => {
                        state.active_panel = match state.active_panel {
                            Panel::Sections => Panel::Settings,
                            Panel::Settings => Panel::Sections,
                        };
                    }

                    // Enter: edit selected setting or switch panel
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Enter,
                        ..
                    }) => {
                        if state.active_panel == Panel::Sections {
                            state.active_panel = Panel::Settings;
                            state.selected_setting = 0;
                            state.settings_scroll_offset = 0;
                        } else if state.active_panel == Panel::Settings {
                            if let Some(row) = state.selected_row() {
                                // Domain-related rows
                                if row.domain_header.is_some()
                                    || row.domain_child.is_some()
                                    || row.field_name == ADD_DOMAIN_FIELD_NAME
                                    || row.field_name.starts_with(DELETE_DOMAIN_FIELD_NAME)
                                {
                                    state.handle_domain_enter(&row);
                                } else if !OverlayState::is_row_editable(&row) {
                                    // FixedByLua: don't allow edits
                                } else {
                                    match &row.kind {
                                        FieldKind::Bool => {
                                            state.toggle_bool(&row.field_name);
                                        }
                                        FieldKind::Enum(variants) => {
                                            let current_str = row
                                                .proposed_value
                                                .as_ref()
                                                .unwrap_or(&row.current_value);
                                            let sel_idx = variants
                                                .iter()
                                                .position(|(v, _)| v == current_str)
                                                .unwrap_or(0);
                                            state.enum_picker = Some(EnumPicker {
                                                field_name: row.field_name.clone(),
                                                variants: variants.clone(),
                                                selected: sel_idx,
                                            });
                                        }
                                        FieldKind::ColorScheme => {
                                            state.open_scheme_picker();
                                        }
                                        FieldKind::Float | FieldKind::Integer | FieldKind::Text => {
                                            let initial = row
                                                .proposed_value
                                                .as_ref()
                                                .unwrap_or(&row.current_value)
                                                .clone();
                                            state.inline_edit = Some(InlineEdit {
                                                field_name: row.field_name.clone(),
                                                buffer: initial,
                                                kind: row.kind.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Space: cycle bool/enum
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char(' '),
                        modifiers: Modifiers::NONE,
                    }) => {
                        if state.active_panel == Panel::Settings {
                            if let Some(row) = state.selected_row() {
                                if let Some(domain_idx) = row.domain_child {
                                    if let Some(field_key) =
                                        extract_domain_field_key(&row.field_name)
                                    {
                                        match &row.kind {
                                            FieldKind::Bool => {
                                                state.toggle_domain_bool(domain_idx, &field_key);
                                            }
                                            FieldKind::Enum(_) => {
                                                state.handle_domain_enter(&row);
                                            }
                                            _ => {}
                                        }
                                    }
                                } else if row.domain_header.is_some() {
                                    state.handle_domain_enter(&row);
                                } else if OverlayState::is_row_editable(&row) {
                                    match &row.kind {
                                        FieldKind::Bool => {
                                            state.toggle_bool(&row.field_name);
                                        }
                                        FieldKind::Enum(_) => {
                                            state.cycle_enum(&row.field_name, 1);
                                        }
                                        FieldKind::ColorScheme => {
                                            state.open_scheme_picker();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // Left/Right: cycle enum or adjust numeric
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::LeftArrow,
                        ..
                    }) => {
                        if state.active_panel == Panel::Settings {
                            if let Some(row) = state.selected_row() {
                                if OverlayState::is_row_editable(&row) {
                                    if let FieldKind::Enum(_) = &row.kind {
                                        state.cycle_enum(&row.field_name, -1);
                                    }
                                }
                            }
                        }
                    }
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::RightArrow,
                        ..
                    }) => {
                        if state.active_panel == Panel::Settings {
                            if let Some(row) = state.selected_row() {
                                if OverlayState::is_row_editable(&row) {
                                    if let FieldKind::Enum(_) = &row.kind {
                                        state.cycle_enum(&row.field_name, 1);
                                    }
                                }
                            }
                        }
                    }

                    // P: preview
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('p'),
                        modifiers: Modifiers::SHIFT,
                    }) => {
                        if !state.proposals.is_empty() {
                            return Ok(ConfigOverlayAction::Preview(state.proposals.clone()));
                        }
                    }

                    // S: save
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('s'),
                        modifiers: Modifiers::SHIFT,
                    }) => {
                        if state.dirty {
                            return Ok(ConfigOverlayAction::Save {
                                proposals: state.proposals.clone(),
                                ssh_domains: state.overlay_domains(),
                            });
                        }
                    }

                    // R: reset selected field
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('r'),
                        modifiers: Modifiers::SHIFT,
                    }) => {
                        if state.active_panel == Panel::Settings {
                            if let Some(row) = state.selected_row() {
                                state.remove_proposal(&row.field_name);
                            }
                        }
                    }

                    // Mouse: click on sections or settings
                    InputEvent::Mouse(MouseEvent {
                        x,
                        y,
                        mouse_buttons,
                        ..
                    }) => {
                        if mouse_buttons.contains(MouseButtons::LEFT) {
                            let size = ratatui_term.backend().size().unwrap_or_default();
                            // Match overlay_rect logic from render.rs
                            let ow = size.width.min(100).max(size.width * 9 / 10).min(size.width);
                            let oh = size
                                .height
                                .min(35)
                                .max(size.height * 9 / 10)
                                .min(size.height);
                            let lp = (size.width.saturating_sub(ow)) / 2;
                            let tp = (size.height.saturating_sub(oh)) / 2;
                            // Body starts after: border(1) + search(1) = row 2 inside overlay
                            let body_y_start = tp + 2;
                            let body_y_end = tp + oh.saturating_sub(7);
                            let sec_x_start = lp + 1;
                            let sec_x_end = sec_x_start + 19; // SECTION_W minus border
                            let set_x_start = sec_x_end + 1;

                            if y >= body_y_start && y < body_y_end {
                                let row = (y - body_y_start) as usize;
                                if x >= sec_x_start && x < sec_x_end {
                                    if row < state.sections.len() {
                                        state.selected_section = row;
                                        state.selected_setting = 0;
                                        state.settings_scroll_offset = 0;
                                        state.active_panel = Panel::Sections;
                                    }
                                } else if x >= set_x_start {
                                    let idx = row + state.settings_scroll_offset;
                                    let count = state.visible_settings().len();
                                    if idx < count {
                                        // If clicking already-selected row, open editor
                                        let was_selected = idx == state.selected_setting
                                            && state.active_panel == Panel::Settings;
                                        state.selected_setting = idx;
                                        state.active_panel = Panel::Settings;
                                        if was_selected {
                                            if let Some(sr) = state.selected_row() {
                                                // Domain rows
                                                if sr.domain_header.is_some()
                                                    || sr.domain_child.is_some()
                                                    || sr.field_name == ADD_DOMAIN_FIELD_NAME
                                                    || sr
                                                        .field_name
                                                        .starts_with(DELETE_DOMAIN_FIELD_NAME)
                                                {
                                                    state.handle_domain_enter(&sr);
                                                } else if OverlayState::is_row_editable(&sr) {
                                                    match &sr.kind {
                                                        FieldKind::Bool => {
                                                            state.toggle_bool(&sr.field_name);
                                                        }
                                                        FieldKind::Enum(variants) => {
                                                            let current_str = sr
                                                                .proposed_value
                                                                .as_ref()
                                                                .unwrap_or(&sr.current_value);
                                                            let sel_idx = variants
                                                                .iter()
                                                                .position(|(v, _)| v == current_str)
                                                                .unwrap_or(0);
                                                            state.enum_picker = Some(EnumPicker {
                                                                field_name: sr.field_name.clone(),
                                                                variants: variants.clone(),
                                                                selected: sel_idx,
                                                            });
                                                        }
                                                        FieldKind::ColorScheme => {
                                                            state.open_scheme_picker();
                                                        }
                                                        FieldKind::Float
                                                        | FieldKind::Integer
                                                        | FieldKind::Text => {
                                                            let initial = sr
                                                                .proposed_value
                                                                .as_ref()
                                                                .unwrap_or(&sr.current_value)
                                                                .clone();
                                                            state.inline_edit = Some(InlineEdit {
                                                                field_name: sr.field_name.clone(),
                                                                buffer: initial,
                                                                kind: sr.kind.clone(),
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Window resize
                    InputEvent::Resized { .. } => {
                        ratatui_term.backend_mut().refresh_size()?;
                        ratatui_term.clear()?;
                    }

                    _ => {}
                }
            }
            Ok(None) => {}
            Err(_) => {
                return Ok(ConfigOverlayAction::Close);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_domain_field_key() {
        assert_eq!(
            extract_domain_field_key("__domain_2_remote_address__"),
            Some("remote_address".to_string())
        );
        assert_eq!(
            extract_domain_field_key("__domain_0_ssh_backend__"),
            Some("ssh_backend".to_string())
        );
        assert_eq!(
            extract_domain_field_key("__domain_10_no_agent_auth__"),
            Some("no_agent_auth".to_string())
        );
        // No underscore after index
        assert_eq!(extract_domain_field_key("__domain___"), None);
    }

    #[test]
    fn test_parse_domain_field_name() {
        let result = parse_domain_field_name("__domain_2_remote_address__");
        assert_eq!(result, Some((2, "remote_address".to_string())));

        let result = parse_domain_field_name("__domain_0_username__");
        assert_eq!(result, Some((0, "username".to_string())));

        // Invalid format
        assert_eq!(parse_domain_field_name("__domain_notanumber_field__"), None);
    }

    #[test]
    fn test_add_domain_field_name() {
        assert_eq!(ADD_DOMAIN_FIELD_NAME, "__add_ssh_domain__");
    }

    #[test]
    fn test_delete_domain_field_name() {
        assert_eq!(DELETE_DOMAIN_FIELD_NAME, "__delete_domain__");
        let name = format!("{}_{}", DELETE_DOMAIN_FIELD_NAME, 3);
        assert!(name.starts_with(DELETE_DOMAIN_FIELD_NAME));
    }

    #[test]
    fn test_domain_header_info_clone() {
        let info = DomainHeaderInfo {
            domain_index: 5,
            source: data::DomainSource::Overlay,
            expanded: true,
        };
        let cloned = info.clone();
        assert_eq!(cloned.domain_index, 5);
        assert_eq!(cloned.source, data::DomainSource::Overlay);
        assert!(cloned.expanded);
    }

    #[test]
    fn test_setting_row_domain_markers() {
        let row = SettingRow {
            field_name: "__domain_header_0__".to_string(),
            display_name: "myhost".to_string(),
            current_value: "myhost:22".to_string(),
            proposed_value: None,
            status: FieldStatus::Inherited,
            kind: FieldKind::Text,
            domain_header: Some(DomainHeaderInfo {
                domain_index: 0,
                source: data::DomainSource::Lua,
                expanded: false,
            }),
            domain_child: None,
        };
        assert!(row.domain_header.is_some());
        assert!(row.domain_child.is_none());
    }

    #[test]
    fn test_config_overlay_action_save_has_domains() {
        let action = ConfigOverlayAction::Save {
            proposals: HashMap::new(),
            ssh_domains: vec![SshDomainConfig {
                name: "test".to_string(),
                remote_address: "host:22".to_string(),
                ..Default::default()
            }],
        };
        match action {
            ConfigOverlayAction::Save {
                proposals,
                ssh_domains,
            } => {
                assert!(proposals.is_empty());
                assert_eq!(ssh_domains.len(), 1);
                assert_eq!(ssh_domains[0].name, "test");
            }
            _ => panic!("Expected Save"),
        }
    }
}
