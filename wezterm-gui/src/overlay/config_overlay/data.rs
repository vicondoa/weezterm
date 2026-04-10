//! Config overlay data definitions — section registry, field metadata, and helpers.
//!
//! --- weezterm remote features ---

use std::collections::HashMap;
use wezterm_dynamic::Value;

/// A section grouping related config fields in the overlay UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Appearance,
    Fonts,
    Tabs,
    Cursor,
    Scrollbar,
    Window,
    Behavior,
    Rendering,
}

impl Section {
    pub fn display_name(&self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Fonts => "Fonts",
            Section::Tabs => "Tabs",
            Section::Cursor => "Cursor",
            Section::Scrollbar => "Scrollbar",
            Section::Window => "Window",
            Section::Behavior => "Behavior",
            Section::Rendering => "Rendering",
        }
    }
}

/// The kind of value a config field holds, for UI editing purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    Float,
    Integer,
    Text,
    /// Enum with a list of known variant display strings.
    Enum(Vec<String>),
}

/// Metadata for a single config field exposed in the overlay.
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// The Rust/Lua config key (e.g. `"font_size"`).
    pub name: &'static str,
    /// Human-readable label shown in the settings panel.
    pub display_name: &'static str,
    /// Which section this field belongs to.
    pub section: Section,
    /// The editing widget to use.
    pub kind: FieldKind,
    /// Brief description shown in the details panel.
    pub doc: &'static str,
}

/// Returns all sections in display order.
pub fn get_sections() -> Vec<Section> {
    vec![
        Section::Appearance,
        Section::Fonts,
        Section::Tabs,
        Section::Cursor,
        Section::Scrollbar,
        Section::Window,
        Section::Behavior,
        Section::Rendering,
    ]
}

/// Returns field definitions for all v1-supported settings.
pub fn get_field_defs() -> Vec<FieldDef> {
    vec![
        // ── Appearance ───────────────────────────────────────────────
        FieldDef {
            name: "color_scheme",
            display_name: "Color Scheme",
            section: Section::Appearance,
            kind: FieldKind::Text,
            doc: "Name of the color scheme to use.",
        },
        FieldDef {
            name: "window_background_opacity",
            display_name: "Window Opacity",
            section: Section::Appearance,
            kind: FieldKind::Float,
            doc: "Background opacity from 0.0 (transparent) to 1.0 (opaque).",
        },
        FieldDef {
            name: "text_background_opacity",
            display_name: "Text Bg Opacity",
            section: Section::Appearance,
            kind: FieldKind::Float,
            doc: "Opacity for the text background layer.",
        },
        FieldDef {
            name: "window_decorations",
            display_name: "Decorations",
            section: Section::Appearance,
            kind: FieldKind::Text,
            doc: "Window decoration flags (e.g. TITLE|RESIZE).",
        },
        FieldDef {
            name: "win32_system_backdrop",
            display_name: "System Backdrop",
            section: Section::Appearance,
            kind: FieldKind::Enum(vec![
                "Auto".into(),
                "Disable".into(),
                "Acrylic".into(),
                "Mica".into(),
                "Tabbed".into(),
            ]),
            doc: "Windows system backdrop effect (Acrylic, Mica, etc.).",
        },
        // ── Fonts ────────────────────────────────────────────────────
        FieldDef {
            name: "font_size",
            display_name: "Font Size",
            section: Section::Fonts,
            kind: FieldKind::Float,
            doc: "Font size measured in points.",
        },
        FieldDef {
            name: "line_height",
            display_name: "Line Height",
            section: Section::Fonts,
            kind: FieldKind::Float,
            doc: "Multiplier for line height (1.0 = normal).",
        },
        FieldDef {
            name: "cell_width",
            display_name: "Cell Width",
            section: Section::Fonts,
            kind: FieldKind::Float,
            doc: "Multiplier for cell width (1.0 = normal).",
        },
        // ── Tabs ─────────────────────────────────────────────────────
        FieldDef {
            name: "enable_tab_bar",
            display_name: "Tab Bar",
            section: Section::Tabs,
            kind: FieldKind::Bool,
            doc: "Show or hide the tab bar.",
        },
        FieldDef {
            name: "use_fancy_tab_bar",
            display_name: "Fancy Tab Bar",
            section: Section::Tabs,
            kind: FieldKind::Bool,
            doc: "Use the fancy rendered tab bar instead of the retro text tab bar.",
        },
        FieldDef {
            name: "hide_tab_bar_if_only_one_tab",
            display_name: "Hide If One Tab",
            section: Section::Tabs,
            kind: FieldKind::Bool,
            doc: "Automatically hide the tab bar when only one tab is open.",
        },
        FieldDef {
            name: "tab_bar_at_bottom",
            display_name: "Tab Bar Bottom",
            section: Section::Tabs,
            kind: FieldKind::Bool,
            doc: "Place the tab bar at the bottom of the window.",
        },
        FieldDef {
            name: "tab_max_width",
            display_name: "Tab Max Width",
            section: Section::Tabs,
            kind: FieldKind::Integer,
            doc: "Maximum width of a tab title in cells.",
        },
        FieldDef {
            name: "show_tab_index_in_tab_bar",
            display_name: "Show Tab Index",
            section: Section::Tabs,
            kind: FieldKind::Bool,
            doc: "Display the tab index number in the tab bar.",
        },
        // ── Cursor ───────────────────────────────────────────────────
        FieldDef {
            name: "default_cursor_style",
            display_name: "Cursor Style",
            section: Section::Cursor,
            kind: FieldKind::Enum(vec![
                "SteadyBlock".into(),
                "BlinkingBlock".into(),
                "SteadyUnderline".into(),
                "BlinkingUnderline".into(),
                "SteadyBar".into(),
                "BlinkingBar".into(),
            ]),
            doc: "Default cursor shape.",
        },
        FieldDef {
            name: "cursor_blink_rate",
            display_name: "Blink Rate",
            section: Section::Cursor,
            kind: FieldKind::Integer,
            doc: "Cursor blink rate in milliseconds (0 = no blink).",
        },
        FieldDef {
            name: "force_reverse_video_cursor",
            display_name: "Reverse Video",
            section: Section::Cursor,
            kind: FieldKind::Bool,
            doc: "Force reverse-video rendering of the cursor.",
        },
        FieldDef {
            name: "animation_fps",
            display_name: "Animation FPS",
            section: Section::Cursor,
            kind: FieldKind::Integer,
            doc: "Frames per second for cursor blink and other animations.",
        },
        // ── Scrollbar ────────────────────────────────────────────────
        FieldDef {
            name: "enable_scroll_bar",
            display_name: "Scroll Bar",
            section: Section::Scrollbar,
            kind: FieldKind::Bool,
            doc: "Show or hide the scroll bar.",
        },
        FieldDef {
            name: "scrollback_lines",
            display_name: "Scrollback Lines",
            section: Section::Scrollbar,
            kind: FieldKind::Integer,
            doc: "Number of lines of scrollback to retain.",
        },
        FieldDef {
            name: "min_scroll_bar_height",
            display_name: "Min Height",
            section: Section::Scrollbar,
            kind: FieldKind::Text,
            doc: "Minimum scroll bar height (e.g. '0.5cell').",
        },
        // ── Window ───────────────────────────────────────────────────
        FieldDef {
            name: "initial_cols",
            display_name: "Initial Cols",
            section: Section::Window,
            kind: FieldKind::Integer,
            doc: "Number of columns in a new window.",
        },
        FieldDef {
            name: "initial_rows",
            display_name: "Initial Rows",
            section: Section::Window,
            kind: FieldKind::Integer,
            doc: "Number of rows in a new window.",
        },
        FieldDef {
            name: "adjust_window_size_when_changing_font_size",
            display_name: "Resize On Font Change",
            section: Section::Window,
            kind: FieldKind::Bool,
            doc: "Resize the window when font size changes.",
        },
        // ── Behavior ─────────────────────────────────────────────────
        FieldDef {
            name: "automatically_reload_config",
            display_name: "Auto Reload Config",
            section: Section::Behavior,
            kind: FieldKind::Bool,
            doc: "Automatically reload config when the file changes.",
        },
        FieldDef {
            name: "audible_bell",
            display_name: "Audible Bell",
            section: Section::Behavior,
            kind: FieldKind::Enum(vec!["SystemBeep".into(), "Disabled".into()]),
            doc: "What to do when the terminal bell rings.",
        },
        // ── Rendering ────────────────────────────────────────────────
        FieldDef {
            name: "front_end",
            display_name: "Front End",
            section: Section::Rendering,
            kind: FieldKind::Enum(vec!["OpenGL".into(), "WebGpu".into(), "Software".into()]),
            doc: "GPU front-end selection.",
        },
        FieldDef {
            name: "webgpu_power_preference",
            display_name: "GPU Power Pref",
            section: Section::Rendering,
            kind: FieldKind::Enum(vec!["LowPower".into(), "HighPerformance".into()]),
            doc: "WebGPU power preference (low power vs high performance).",
        },
        FieldDef {
            name: "max_fps",
            display_name: "Max FPS",
            section: Section::Rendering,
            kind: FieldKind::Integer,
            doc: "Maximum frames per second for rendering.",
        },
    ]
}

/// Convert a `Value` to a short display string.
pub fn value_to_display_string(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "On" } else { "Off" }.to_string(),
        Value::String(s) => s.clone(),
        Value::I64(i) => i.to_string(),
        Value::U64(u) => u.to_string(),
        Value::F64(f) => {
            let f = f64::from(*f);
            if f == f.floor() && f.abs() < 1e9 {
                format!("{:.0}", f)
            } else {
                format!("{:.2}", f)
            }
        }
        Value::Null => "—".to_string(),
        _ => "…".to_string(),
    }
}

/// Compare two `Value`s for display equality (ignoring numeric type differences).
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::I64(a), Value::I64(b)) => a == b,
        (Value::U64(a), Value::U64(b)) => a == b,
        (Value::F64(a), Value::F64(b)) => a == b,
        // Cross-type numeric comparison
        (Value::I64(a), Value::F64(b)) => (*a as f64) == f64::from(*b),
        (Value::F64(a), Value::I64(b)) => f64::from(*a) == (*b as f64),
        (Value::U64(a), Value::F64(b)) => (*a as f64) == f64::from(*b),
        (Value::F64(a), Value::U64(b)) => f64::from(*a) == (*b as f64),
        (Value::I64(a), Value::U64(b)) => *a >= 0 && (*a as u64) == *b,
        (Value::U64(a), Value::I64(b)) => *b >= 0 && *a == (*b as u64),
        _ => false,
    }
}

/// Extract values for all known field defs from a config dynamic value.
pub fn extract_values(config_value: &Value) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    if let Value::Object(obj) = config_value {
        for field in get_field_defs() {
            if let Some(v) = obj.get_by_str(field.name) {
                map.insert(field.name.to_string(), v.clone());
            }
        }
    }
    map
}
