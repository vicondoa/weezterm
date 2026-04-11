//! Config overlay data definitions — section registry, field metadata, and helpers.
//!
//! --- weezterm remote features ---

use std::collections::HashMap;
use wezterm_dynamic::Value;

/// A section grouping related config fields in the overlay UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    General,
    FontAndText,
    TabsAndPanes,
    CursorAndAnimation,
    Terminal,
    Input,
    SshAndDomains,
    Rendering,
}

impl Section {
    pub fn display_name(&self) -> &'static str {
        match self {
            Section::General => "General",
            Section::FontAndText => "Font & Text",
            Section::TabsAndPanes => "Tabs & Panes",
            Section::CursorAndAnimation => "Cursor",
            Section::Terminal => "Terminal",
            Section::Input => "Input",
            Section::SshAndDomains => "SSH & Domains",
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
    Enum(Vec<String>),
}

/// Metadata for a single config field exposed in the overlay.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: &'static str,
    pub display_name: &'static str,
    pub section: Section,
    pub kind: FieldKind,
    pub doc: &'static str,
}

/// Returns all sections in display order.
pub fn get_sections() -> Vec<Section> {
    vec![
        Section::General,
        Section::FontAndText,
        Section::TabsAndPanes,
        Section::CursorAndAnimation,
        Section::Terminal,
        Section::Input,
        Section::SshAndDomains,
        Section::Rendering,
    ]
}

/// Returns field definitions for all supported settings.
pub fn get_field_defs() -> Vec<FieldDef> {
    use FieldKind::*;
    use Section::*;
    vec![
        // ── General ──────────────────────────────────────────────────
        f(
            "color_scheme",
            "Color Scheme",
            General,
            Text,
            "Name of the color scheme to use",
        ),
        f(
            "window_background_opacity",
            "Window Opacity",
            General,
            Float,
            "Background opacity 0.0–1.0",
        ),
        f(
            "text_background_opacity",
            "Text Bg Opacity",
            General,
            Float,
            "Text background opacity 0.0–1.0",
        ),
        f(
            "window_decorations",
            "Window Decorations",
            General,
            Text,
            "Decoration flags (TITLE|RESIZE)",
        ),
        f(
            "win32_system_backdrop",
            "System Backdrop",
            General,
            Enum(sv(&["Auto", "Disable", "Acrylic", "Mica", "Tabbed"])),
            "Windows backdrop effect",
        ),
        f(
            "bold_brightens_ansi_colors",
            "Bold Brightens ANSI",
            General,
            Enum(sv(&["BrightAndBold", "BrightOnly", "No"])),
            "Bold text color handling",
        ),
        f(
            "default_domain",
            "Default Domain",
            General,
            Text,
            "Default multiplexer domain",
        ),
        f(
            "default_workspace",
            "Default Workspace",
            General,
            Text,
            "Default workspace name",
        ),
        f(
            "automatically_reload_config",
            "Auto Reload Config",
            General,
            Bool,
            "Reload config on file change",
        ),
        f(
            "check_for_updates",
            "Check for Updates",
            General,
            Bool,
            "Periodically check for updates",
        ),
        f(
            "quit_when_all_windows_are_closed",
            "Quit When All Closed",
            General,
            Bool,
            "Exit when last window closes",
        ),
        f(
            "window_close_confirmation",
            "Close Confirmation",
            General,
            Enum(sv(&["AlwaysPrompt", "NeverPrompt"])),
            "Prompt before closing window",
        ),
        // ── Font & Text ──────────────────────────────────────────────
        f(
            "font_size",
            "Font Size",
            FontAndText,
            Float,
            "Font size in points",
        ),
        f(
            "line_height",
            "Line Height",
            FontAndText,
            Float,
            "Line height multiplier (1.0 = normal)",
        ),
        f(
            "cell_width",
            "Cell Width",
            FontAndText,
            Float,
            "Cell width multiplier (1.0 = normal)",
        ),
        f(
            "command_palette_font_size",
            "Palette Font Size",
            FontAndText,
            Float,
            "Command palette font size",
        ),
        f(
            "char_select_font_size",
            "Char Select Font Size",
            FontAndText,
            Float,
            "Character selector font size",
        ),
        f(
            "warn_about_missing_glyphs",
            "Warn Missing Glyphs",
            FontAndText,
            Bool,
            "Log warnings for missing glyphs",
        ),
        f(
            "custom_block_glyphs",
            "Custom Block Glyphs",
            FontAndText,
            Bool,
            "Use built-in block/box glyphs",
        ),
        f(
            "anti_alias_custom_block_glyphs",
            "AA Block Glyphs",
            FontAndText,
            Bool,
            "Anti-alias custom block glyphs",
        ),
        f(
            "font_locator",
            "Font Locator",
            FontAndText,
            Enum(sv(&["ConfigDirsOnly"])),
            "How fonts are discovered",
        ),
        f(
            "font_shaper",
            "Font Shaper",
            FontAndText,
            Enum(sv(&["Harfbuzz", "Allsorts"])),
            "Text shaping engine",
        ),
        f(
            "freetype_load_target",
            "FreeType Target",
            FontAndText,
            Enum(sv(&["Normal", "Light", "Mono", "HorizontalLcd"])),
            "FreeType hinting mode",
        ),
        f(
            "display_pixel_geometry",
            "Pixel Geometry",
            FontAndText,
            Enum(sv(&["RGB", "BGR", "Horizontal", "Vertical"])),
            "Sub-pixel rendering order",
        ),
        f(
            "sort_fallback_fonts_by_coverage",
            "Sort Fallback Fonts",
            FontAndText,
            Bool,
            "Sort fallback fonts by glyph coverage",
        ),
        // ── Tabs & Panes ─────────────────────────────────────────────
        f(
            "enable_tab_bar",
            "Tab Bar",
            TabsAndPanes,
            Bool,
            "Show or hide the tab bar",
        ),
        f(
            "use_fancy_tab_bar",
            "Fancy Tab Bar",
            TabsAndPanes,
            Bool,
            "Use rendered tab bar vs retro text",
        ),
        f(
            "hide_tab_bar_if_only_one_tab",
            "Hide If One Tab",
            TabsAndPanes,
            Bool,
            "Auto-hide tab bar with one tab",
        ),
        f(
            "tab_bar_at_bottom",
            "Tab Bar at Bottom",
            TabsAndPanes,
            Bool,
            "Place tab bar at window bottom",
        ),
        f(
            "tab_max_width",
            "Tab Max Width",
            TabsAndPanes,
            Integer,
            "Maximum tab title width in cells",
        ),
        f(
            "show_tab_index_in_tab_bar",
            "Show Tab Index",
            TabsAndPanes,
            Bool,
            "Display tab index numbers",
        ),
        f(
            "show_tabs_in_tab_bar",
            "Show Tabs",
            TabsAndPanes,
            Bool,
            "Show tab entries in tab bar",
        ),
        f(
            "show_new_tab_button_in_tab_bar",
            "New Tab Button",
            TabsAndPanes,
            Bool,
            "Show + button in tab bar",
        ),
        f(
            "show_close_tab_button_in_tabs",
            "Close Tab Button",
            TabsAndPanes,
            Bool,
            "Show × button on tabs",
        ),
        f(
            "tab_and_split_indices_are_zero_based",
            "Zero-Based Indices",
            TabsAndPanes,
            Bool,
            "Use 0-based tab/pane indices",
        ),
        f(
            "switch_to_last_active_tab_when_closing_tab",
            "Switch to Last Tab",
            TabsAndPanes,
            Bool,
            "Go to previous tab on close",
        ),
        f(
            "unzoom_on_switch_pane",
            "Unzoom on Switch",
            TabsAndPanes,
            Bool,
            "Unzoom when switching panes",
        ),
        f(
            "pane_focus_follows_mouse",
            "Pane Focus Follows Mouse",
            TabsAndPanes,
            Bool,
            "Focus pane under mouse cursor",
        ),
        // ── Cursor & Animation ───────────────────────────────────────
        f(
            "default_cursor_style",
            "Cursor Style",
            CursorAndAnimation,
            Enum(sv(&[
                "SteadyBlock",
                "BlinkingBlock",
                "SteadyUnderline",
                "BlinkingUnderline",
                "SteadyBar",
                "BlinkingBar",
            ])),
            "Default cursor shape",
        ),
        f(
            "cursor_blink_rate",
            "Cursor Blink Rate",
            CursorAndAnimation,
            Integer,
            "Blink rate in ms (0 = no blink)",
        ),
        f(
            "force_reverse_video_cursor",
            "Reverse Video Cursor",
            CursorAndAnimation,
            Bool,
            "Force reverse-video cursor",
        ),
        f(
            "animation_fps",
            "Animation FPS",
            CursorAndAnimation,
            Integer,
            "Frames/sec for animations",
        ),
        f(
            "text_blink_rate",
            "Text Blink Rate",
            CursorAndAnimation,
            Integer,
            "Text blink rate in ms",
        ),
        f(
            "text_blink_rate_rapid",
            "Rapid Blink Rate",
            CursorAndAnimation,
            Integer,
            "Rapid text blink rate in ms",
        ),
        // ── Terminal ─────────────────────────────────────────────────
        f(
            "term",
            "TERM Variable",
            Terminal,
            Text,
            "Value of $TERM environment variable",
        ),
        f(
            "scrollback_lines",
            "Scrollback Lines",
            Terminal,
            Integer,
            "Lines of scrollback to retain",
        ),
        f(
            "initial_cols",
            "Initial Columns",
            Terminal,
            Integer,
            "Columns in new windows",
        ),
        f(
            "initial_rows",
            "Initial Rows",
            Terminal,
            Integer,
            "Rows in new windows",
        ),
        f(
            "exit_behavior",
            "Exit Behavior",
            Terminal,
            Enum(sv(&["Close", "Hold", "CloseOnCleanExit"])),
            "What happens when shell exits",
        ),
        f(
            "scroll_to_bottom_on_input",
            "Scroll on Input",
            Terminal,
            Bool,
            "Scroll to bottom when typing",
        ),
        f(
            "detect_password_input",
            "Detect Passwords",
            Terminal,
            Bool,
            "Detect password prompts",
        ),
        f(
            "enable_kitty_graphics",
            "Kitty Graphics",
            Terminal,
            Bool,
            "Support Kitty image protocol",
        ),
        f(
            "enable_kitty_keyboard",
            "Kitty Keyboard",
            Terminal,
            Bool,
            "Support Kitty keyboard protocol",
        ),
        f(
            "enable_csi_u_key_encoding",
            "CSI-u Keys",
            Terminal,
            Bool,
            "Enable CSI-u key encoding",
        ),
        f(
            "use_ime",
            "Use IME",
            Terminal,
            Bool,
            "Enable input method editor",
        ),
        f(
            "use_dead_keys",
            "Use Dead Keys",
            Terminal,
            Bool,
            "Enable dead key composition",
        ),
        f(
            "normalize_output_to_unicode_nfc",
            "Normalize to NFC",
            Terminal,
            Bool,
            "Normalize terminal output to NFC",
        ),
        f(
            "enable_scroll_bar",
            "Scroll Bar",
            Terminal,
            Bool,
            "Show scrollbar",
        ),
        f(
            "alternate_buffer_wheel_scroll_speed",
            "Alt Buffer Scroll",
            Terminal,
            Integer,
            "Mouse wheel speed in alt buffer",
        ),
        // ── Input ────────────────────────────────────────────────────
        f(
            "swap_backspace_and_delete",
            "Swap Backspace/Delete",
            Input,
            Bool,
            "Swap Backspace and Delete keys",
        ),
        f(
            "disable_default_key_bindings",
            "Disable Default Keys",
            Input,
            Bool,
            "Disable built-in key bindings",
        ),
        f(
            "disable_default_mouse_bindings",
            "Disable Default Mouse",
            Input,
            Bool,
            "Disable built-in mouse bindings",
        ),
        f(
            "hide_mouse_cursor_when_typing",
            "Hide Mouse on Type",
            Input,
            Bool,
            "Hide cursor while typing",
        ),
        f(
            "mouse_wheel_scrolls_tabs",
            "Wheel Scrolls Tabs",
            Input,
            Bool,
            "Mouse wheel switches tabs",
        ),
        f(
            "swallow_mouse_click_on_pane_focus",
            "Swallow Pane Click",
            Input,
            Bool,
            "Eat click that focuses a pane",
        ),
        f(
            "swallow_mouse_click_on_window_focus",
            "Swallow Window Click",
            Input,
            Bool,
            "Eat click that focuses window",
        ),
        f(
            "debug_key_events",
            "Debug Key Events",
            Input,
            Bool,
            "Log key events for debugging",
        ),
        f(
            "key_map_preference",
            "Key Map Preference",
            Input,
            Enum(sv(&["Mapped", "Physical"])),
            "Use mapped or physical key layout",
        ),
        // ── SSH & Domains ────────────────────────────────────────────
        f(
            "ssh_backend",
            "SSH Backend",
            SshAndDomains,
            Enum(sv(&["Ssh2", "LibSsh"])),
            "SSH implementation to use",
        ),
        f(
            "mux_enable_ssh_agent",
            "SSH Agent",
            SshAndDomains,
            Bool,
            "Enable SSH agent forwarding",
        ),
        f(
            "default_ssh_auth_sock",
            "SSH Auth Socket",
            SshAndDomains,
            Text,
            "Path to SSH auth socket",
        ),
        f(
            "default_mux_server_domain",
            "Mux Server Domain",
            SshAndDomains,
            Text,
            "Default mux server domain name",
        ),
        // ── Rendering ────────────────────────────────────────────────
        f(
            "front_end",
            "Front End",
            Rendering,
            Enum(sv(&["OpenGL", "WebGpu", "Software"])),
            "GPU rendering backend",
        ),
        f(
            "webgpu_power_preference",
            "GPU Power Pref",
            Rendering,
            Enum(sv(&["LowPower", "HighPerformance"])),
            "WebGPU power vs performance",
        ),
        f(
            "max_fps",
            "Max FPS",
            Rendering,
            Integer,
            "Maximum rendering frame rate",
        ),
        f(
            "prefer_egl",
            "Prefer EGL",
            Rendering,
            Bool,
            "Prefer EGL over other GL",
        ),
        f(
            "webgpu_force_fallback_adapter",
            "Force GPU Fallback",
            Rendering,
            Bool,
            "Force WebGPU software fallback",
        ),
        f(
            "adjust_window_size_when_changing_font_size",
            "Resize on Font Change",
            Rendering,
            Bool,
            "Resize window when font changes",
        ),
        f(
            "use_resize_increments",
            "Resize Increments",
            Rendering,
            Bool,
            "Snap window to cell boundaries",
        ),
        f(
            "experimental_pixel_positioning",
            "Pixel Positioning",
            Rendering,
            Bool,
            "Experimental sub-cell positioning",
        ),
    ]
}

fn f(
    name: &'static str,
    display_name: &'static str,
    section: Section,
    kind: FieldKind,
    doc: &'static str,
) -> FieldDef {
    FieldDef {
        name,
        display_name,
        section,
        kind,
        doc,
    }
}

fn sv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// Enrich field definitions with documentation from `ConfigMeta` (extracted
/// from `///` doc comments on the `Config` struct fields).
///
/// The `doc` field in each `FieldDef` is used as a fallback if ConfigMeta
/// has no documentation for that field (empty string).
pub fn enrich_docs_from_config(defs: &mut [FieldDef], config: &config::Config) {
    use config::meta::ConfigMeta;
    let options = config.get_config_options();
    for field in defs.iter_mut() {
        if let Some(opt) = options.iter().find(|o| o.name == field.name) {
            let meta_doc = opt.doc.trim();
            if !meta_doc.is_empty() {
                // Take just the first line/sentence for brevity
                let first_line = meta_doc.lines().next().unwrap_or(meta_doc).trim();
                if !first_line.is_empty() {
                    field.doc = first_line;
                }
            }
        }
    }
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
        Value::Null => "-".to_string(),
        _ => "...".to_string(),
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
