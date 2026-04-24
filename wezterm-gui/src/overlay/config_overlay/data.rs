//! Config overlay data definitions — section registry, field metadata, and helpers.
//!
//! --- weezterm remote features ---

use std::collections::HashMap;
use wezterm_dynamic::Value;

/// A user-managed SSH domain configuration (simplified subset of config::SshDomain).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshDomainConfig {
    pub name: String,
    pub remote_address: String,
    pub username: String,
    pub multiplexing: String,
    pub ssh_backend: String,
    pub no_agent_auth: bool,
    pub connect_automatically: bool,
}

impl Default for SshDomainConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            remote_address: String::new(),
            username: String::new(),
            multiplexing: "None".to_string(),
            ssh_backend: "LibSsh".to_string(),
            no_agent_auth: false,
            connect_automatically: false,
        }
    }
}

/// Source of a domain entry — from Lua config or from the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainSource {
    /// Domain is defined in the Lua config file (read-only).
    Lua,
    /// Domain is managed by the overlay (editable).
    Overlay,
}

/// A domain entry with its source and config.
#[derive(Debug, Clone)]
pub struct DomainEntry {
    pub config: SshDomainConfig,
    pub source: DomainSource,
    pub expanded: bool,
}

/// The per-domain fields shown when a domain group is expanded.
pub fn domain_field_defs() -> Vec<(&'static str, &'static str, FieldKind, &'static str)> {
    use FieldKind::*;
    vec![
        (
            "remote_address",
            "Remote Address",
            Text,
            "host:port of the remote server",
        ),
        ("username", "Username", Text, "SSH username"),
        (
            "multiplexing",
            "Multiplexing",
            Enum(ev(&[
                ("None", "Direct SSH connection (no mux server)"),
                ("WezTerm", "Use WezTerm mux server on remote"),
            ])),
            "SSH multiplexing mode",
        ),
        (
            "ssh_backend",
            "SSH Backend",
            Enum(ev(&[
                ("LibSsh", "Use the libssh library (default)"),
                ("Ssh2", "Use the libssh2 library"),
            ])),
            "SSH implementation to use",
        ),
        (
            "no_agent_auth",
            "Disable Agent Auth",
            Bool,
            "Disable SSH agent authentication",
        ),
        (
            "connect_automatically",
            "Auto Connect",
            Bool,
            "Connect to this domain at startup",
        ),
    ]
}

/// Read a field value from an SshDomainConfig by field name.
pub fn domain_field_value(config: &SshDomainConfig, field: &str) -> String {
    match field {
        "remote_address" => config.remote_address.clone(),
        "username" => config.username.clone(),
        "multiplexing" => config.multiplexing.clone(),
        "ssh_backend" => config.ssh_backend.clone(),
        "no_agent_auth" => if config.no_agent_auth { "On" } else { "Off" }.to_string(),
        "connect_automatically" => if config.connect_automatically {
            "On"
        } else {
            "Off"
        }
        .to_string(),
        _ => String::new(),
    }
}

/// Write a field value to an SshDomainConfig by field name.
pub fn set_domain_field(config: &mut SshDomainConfig, field: &str, value: &str) {
    match field {
        "remote_address" => config.remote_address = value.to_string(),
        "username" => config.username = value.to_string(),
        "multiplexing" => config.multiplexing = value.to_string(),
        "ssh_backend" => config.ssh_backend = value.to_string(),
        "no_agent_auth" => config.no_agent_auth = value == "On" || value == "true",
        "connect_automatically" => config.connect_automatically = value == "On" || value == "true",
        _ => {}
    }
}

/// Build SshDomainConfig entries from the Lua config's ssh_domains.
pub fn domains_from_config() -> Vec<DomainEntry> {
    let config = config::configuration();
    let ssh_domains = config.ssh_domains();
    ssh_domains
        .into_iter()
        .map(|dom| DomainEntry {
            config: SshDomainConfig {
                name: dom.name.clone(),
                remote_address: dom.remote_address.clone(),
                username: dom.username.clone().unwrap_or_default(),
                multiplexing: format!("{:?}", dom.multiplexing),
                ssh_backend: dom
                    .ssh_backend
                    .map(|b| format!("{:?}", b))
                    .unwrap_or_else(|| "LibSsh".to_string()),
                no_agent_auth: dom.no_agent_auth,
                connect_automatically: dom.connect_automatically,
            },
            source: DomainSource::Lua,
            expanded: false,
        })
        .collect()
}

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
    // --- weezterm remote features ---
    Monitors,
    // --- end weezterm remote features ---
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
            Section::SshAndDomains => "Domains",
            Section::Rendering => "Rendering",
            // --- weezterm remote features ---
            Section::Monitors => "Monitors",
            // --- end weezterm remote features ---
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
    /// Enum with (variant_name, description) pairs.
    Enum(Vec<(String, String)>),
    /// Color scheme selector — shows a searchable picker with color previews.
    ColorScheme,
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
        // --- weezterm remote features ---
        Section::Monitors,
        // --- end weezterm remote features ---
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
            ColorScheme,
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
            Enum(ev(&[
                ("Auto", "System decides the effect"),
                ("Disable", "No backdrop effect"),
                ("Acrylic", "Translucent blurred background"),
                ("Mica", "Subtle tinted desktop wallpaper"),
                ("Tabbed", "Mica variant for tabbed windows"),
            ])),
            "Windows backdrop effect",
        ),
        f(
            "bold_brightens_ansi_colors",
            "Bold Brightens ANSI",
            General,
            Enum(ev(&[
                ("BrightAndBold", "Brighten color and use bold font"),
                ("BrightOnly", "Brighten color but keep normal weight"),
                ("No", "Do not brighten bold text"),
            ])),
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
            Enum(ev(&[
                ("AlwaysPrompt", "Always ask before closing"),
                ("NeverPrompt", "Close without asking"),
            ])),
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
            Enum(ev(&[
                ("SteadyBlock", "Non-blinking filled block"),
                ("BlinkingBlock", "Blinking filled block"),
                ("SteadyUnderline", "Non-blinking underline"),
                ("BlinkingUnderline", "Blinking underline"),
                ("SteadyBar", "Non-blinking vertical bar"),
                ("BlinkingBar", "Blinking vertical bar"),
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
            Enum(ev(&[
                ("Close", "Close the pane immediately"),
                ("Hold", "Keep pane open after exit"),
                ("CloseOnCleanExit", "Close only on exit code 0"),
            ])),
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
            Enum(ev(&[
                ("Mapped", "Use the OS keyboard layout mapping"),
                ("Physical", "Use raw physical key positions"),
            ])),
            "Use mapped or physical key layout",
        ),
        // ── SSH & Domains ────────────────────────────────────────────
        f(
            "ssh_backend",
            "SSH Backend",
            SshAndDomains,
            Enum(ev(&[
                ("Ssh2", "Use the libssh2 library"),
                ("LibSsh", "Use the libssh library (default)"),
            ])),
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
            Enum(ev(&[
                ("OpenGL", "Hardware-accelerated OpenGL rendering"),
                ("WebGpu", "Modern WebGPU rendering backend"),
                ("Software", "CPU-based software rendering"),
            ])),
            "GPU rendering backend",
        ),
        f(
            "webgpu_power_preference",
            "GPU Power Pref",
            Rendering,
            Enum(ev(&[
                ("LowPower", "Prefer battery life over performance"),
                ("HighPerformance", "Prefer performance over battery"),
            ])),
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

fn sv(items: &[&str]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|s| ((*s).to_string(), String::new()))
        .collect()
}

/// Build enum variants with descriptions.
fn ev(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(v, d)| ((*v).to_string(), (*d).to_string()))
        .collect()
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
        Value::Null => String::new(),
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

/// Get sorted color scheme names with their palettes for the color scheme picker.
pub fn get_color_schemes() -> Vec<(String, config::Palette)> {
    let builtin = &config::COLOR_SCHEMES;
    let user_config = config::configuration();

    let mut schemes: Vec<(String, config::Palette)> = builtin
        .iter()
        .map(|(name, palette)| (name.clone(), palette.clone()))
        .collect();

    // Add user-defined schemes from config
    for (name, palette) in &user_config.color_schemes {
        if !schemes.iter().any(|(n, _)| n == name) {
            schemes.push((name.clone(), palette.clone()));
        }
    }

    schemes.sort_by(|(a, _), (b, _)| a.to_lowercase().cmp(&b.to_lowercase()));
    schemes
}

/// Convert an overlay SshDomainConfig to a config::SshDomain for runtime registration.
pub fn to_config_ssh_domain(dom: &SshDomainConfig) -> config::SshDomain {
    use config::{SshBackend, SshMultiplexing};
    let multiplexing = match dom.multiplexing.as_str() {
        "WezTerm" => SshMultiplexing::WezTerm,
        _ => SshMultiplexing::None,
    };
    let ssh_backend = match dom.ssh_backend.as_str() {
        "Ssh2" => Some(SshBackend::Ssh2),
        "LibSsh" => Some(SshBackend::LibSsh),
        _ => None,
    };
    config::SshDomain {
        name: dom.name.clone(),
        remote_address: dom.remote_address.clone(),
        username: if dom.username.is_empty() {
            None
        } else {
            Some(dom.username.clone())
        },
        multiplexing,
        ssh_backend,
        no_agent_auth: dom.no_agent_auth,
        connect_automatically: dom.connect_automatically,
        ..Default::default()
    }
}

// --- weezterm remote features ---
/// A monitor override entry for the config overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorOverrideEntry {
    /// The monitor name (from ScreenInfo).
    pub monitor_name: String,
    /// The assigned color scheme, or None for "use default".
    pub color_scheme: Option<String>,
    /// Whether this monitor is the one the current window is on.
    pub is_current: bool,
    /// Whether this entry is expanded in the overlay UI.
    pub expanded: bool,
    /// Screen position and size in pixels (for layout diagram).
    pub screen_rect: Option<MonitorRect>,
}

/// Screen rectangle for layout diagram rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: isize,
    pub y: isize,
    pub width: isize,
    pub height: isize,
}

/// Build MonitorOverrideEntry list from the current system monitors
/// and the config's monitor_overrides settings.
///
/// This must be called from code that has access to the screen list
/// (typically the GUI thread). Pass the result to the config overlay.
pub fn monitors_from_config_with_screens(
    screen_info: &std::collections::HashMap<String, window::screen::ScreenInfo>,
    current_screen: Option<&str>,
) -> Vec<MonitorOverrideEntry> {
    let config = config::configuration();

    let mut entries: Vec<MonitorOverrideEntry> = Vec::new();
    for (name, info) in screen_info {
        let color_scheme = config
            .monitor_overrides
            .iter()
            .find(|mo| &mo.monitor == name)
            .and_then(|mo| mo.color_scheme.clone());

        let rect = info.rect;
        entries.push(MonitorOverrideEntry {
            monitor_name: name.clone(),
            color_scheme,
            is_current: current_screen == Some(name.as_str()),
            expanded: false,
            screen_rect: Some(MonitorRect {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            }),
        });
    }

    // Also include any configured monitors that aren't currently connected
    for mo in &config.monitor_overrides {
        if !entries.iter().any(|e| e.monitor_name == mo.monitor) {
            entries.push(MonitorOverrideEntry {
                monitor_name: mo.monitor.clone(),
                color_scheme: mo.color_scheme.clone(),
                is_current: false,
                expanded: false,
                screen_rect: None,
            });
        }
    }

    entries.sort_by(|a, b| a.monitor_name.cmp(&b.monitor_name));
    entries
}

/// Convert MonitorOverrideEntry list to config::MonitorOverride list
/// (only entries with at least one override set).
pub fn to_config_monitor_overrides(
    entries: &[MonitorOverrideEntry],
) -> Vec<config::MonitorOverride> {
    entries
        .iter()
        .filter(|e| e.color_scheme.is_some())
        .map(|e| config::MonitorOverride {
            monitor: e.monitor_name.clone(),
            color_scheme: e.color_scheme.clone(),
        })
        .collect()
}
// --- end weezterm remote features ---

// --- weezterm remote features ---

/// A user-managed DevContainer domain configuration for overlay editing.
/// Embeds a full `SshDomainConfig` so all SSH options are available and
/// reuse the same field definitions / getters / setters — no drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevContainerOverlayConfig {
    pub name: String,
    /// Full SSH configuration (reuses the same type as SSH domains).
    /// When all SSH fields are empty/default, the domain uses local Docker.
    pub ssh: SshDomainConfig,
    pub default_workspace_folder: String,
    pub default_container: String,
    pub docker_command: String,
    pub devcontainer_command: String,
    pub default_shell: String,
    pub override_user: String,
    pub poll_interval_secs: String,
    pub auto_discover: bool,
}

impl Default for DevContainerOverlayConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            ssh: SshDomainConfig::default(),
            default_workspace_folder: String::new(),
            default_container: String::new(),
            docker_command: "docker".to_string(),
            devcontainer_command: "devcontainer".to_string(),
            default_shell: String::new(),
            override_user: String::new(),
            poll_interval_secs: "10".to_string(),
            auto_discover: true,
        }
    }
}

/// A DevContainer domain entry with source tracking.
#[derive(Debug, Clone)]
pub struct DevContainerEntry {
    pub config: DevContainerOverlayConfig,
    pub source: DomainSource,
    pub expanded: bool,
}

/// Field definitions for devcontainer-specific settings (NOT the SSH sub-fields).
/// SSH fields are provided by `domain_field_defs()` and rendered as a nested group.
pub fn devcontainer_own_field_defs() -> Vec<(&'static str, &'static str, FieldKind, &'static str)> {
    use FieldKind::*;
    vec![
        (
            "default_workspace_folder",
            "DevContainer Workspace",
            Text,
            "Workspace folder on host to auto-match",
        ),
        (
            "default_container",
            "DevContainer Default",
            Text,
            "Container name/ID to auto-connect to",
        ),
        (
            "docker_command",
            "DevContainer Docker Cmd",
            Text,
            "Path to the Docker CLI",
        ),
        (
            "devcontainer_command",
            "DevContainer CLI",
            Text,
            "Path to the devcontainer CLI",
        ),
        (
            "default_shell",
            "DevContainer Shell",
            Text,
            "Shell inside containers (empty = auto-detect)",
        ),
        (
            "override_user",
            "DevContainer User",
            Text,
            "Override container default user (empty = use container default)",
        ),
        (
            "poll_interval_secs",
            "DevContainer Poll (s)",
            Integer,
            "Seconds between container discovery polls",
        ),
        (
            "auto_discover",
            "DevContainer Auto Discover",
            Bool,
            "Discover running devcontainers on domain attach",
        ),
    ]
}

/// Read a devcontainer-own field value by name.
pub fn devcontainer_field_value(config: &DevContainerOverlayConfig, field: &str) -> String {
    match field {
        "default_workspace_folder" => config.default_workspace_folder.clone(),
        "default_container" => config.default_container.clone(),
        "docker_command" => config.docker_command.clone(),
        "devcontainer_command" => config.devcontainer_command.clone(),
        "default_shell" => config.default_shell.clone(),
        "override_user" => config.override_user.clone(),
        "poll_interval_secs" => config.poll_interval_secs.clone(),
        "auto_discover" => if config.auto_discover { "On" } else { "Off" }.to_string(),
        // Delegate SSH fields to the shared getter
        _ => domain_field_value(&config.ssh, field),
    }
}

/// Write a devcontainer field value by name.
pub fn set_devcontainer_field(config: &mut DevContainerOverlayConfig, field: &str, value: &str) {
    match field {
        "default_workspace_folder" => config.default_workspace_folder = value.to_string(),
        "default_container" => config.default_container = value.to_string(),
        "docker_command" => config.docker_command = value.to_string(),
        "devcontainer_command" => config.devcontainer_command = value.to_string(),
        "default_shell" => config.default_shell = value.to_string(),
        "override_user" => config.override_user = value.to_string(),
        "poll_interval_secs" => config.poll_interval_secs = value.to_string(),
        "auto_discover" => config.auto_discover = value == "On" || value == "true",
        // Delegate SSH fields to the shared setter
        _ => set_domain_field(&mut config.ssh, field, value),
    }
}

/// Returns ALL field definitions for a devcontainer domain: own fields + SSH fields.
/// Used by `visible_settings()` to build the flattened child row list.
pub fn devcontainer_all_field_defs() -> Vec<(&'static str, &'static str, FieldKind, &'static str)> {
    let mut fields = devcontainer_own_field_defs();
    // Append the full SSH domain fields — same defs, no duplication
    fields.extend(domain_field_defs());
    fields
}

pub fn devcontainers_from_config() -> Vec<DevContainerEntry> {
    let config = config::configuration();
    config
        .devcontainer_domains
        .iter()
        .map(|dc| {
            let ssh = if let Some(ref s) = dc.ssh {
                SshDomainConfig {
                    name: s.name.clone(),
                    remote_address: s.remote_address.clone(),
                    username: s.username.clone().unwrap_or_default(),
                    multiplexing: format!("{:?}", s.multiplexing),
                    ssh_backend: s
                        .ssh_backend
                        .map(|b| format!("{:?}", b))
                        .unwrap_or_else(|| "LibSsh".to_string()),
                    no_agent_auth: s.no_agent_auth,
                    connect_automatically: s.connect_automatically,
                }
            } else {
                SshDomainConfig::default()
            };
            DevContainerEntry {
                config: DevContainerOverlayConfig {
                    name: dc.name.clone(),
                    ssh,
                    default_workspace_folder: dc
                        .default_workspace_folder
                        .clone()
                        .unwrap_or_default(),
                    default_container: dc.default_container.clone().unwrap_or_default(),
                    docker_command: dc.docker_command.clone(),
                    devcontainer_command: dc.devcontainer_command.clone(),
                    default_shell: dc.default_shell.clone().unwrap_or_default(),
                    override_user: dc.override_user.clone().unwrap_or_default(),
                    poll_interval_secs: dc.poll_interval_secs.to_string(),
                    auto_discover: dc.auto_discover,
                },
                source: DomainSource::Lua,
                expanded: false,
            }
        })
        .collect()
}

pub fn to_config_devcontainer_domain(
    dc: &DevContainerOverlayConfig,
) -> config::devcontainer::DevContainerDomainConfig {
    // Convert the embedded SSH config back to runtime SshDomain
    let ssh = if dc.ssh.remote_address.is_empty() {
        None
    } else {
        let mut ssh_dom = to_config_ssh_domain(&dc.ssh);
        ssh_dom.name = dc.name.clone();
        Some(ssh_dom)
    };
    config::devcontainer::DevContainerDomainConfig {
        name: dc.name.clone(),
        ssh,
        default_workspace_folder: if dc.default_workspace_folder.is_empty() {
            None
        } else {
            Some(dc.default_workspace_folder.clone())
        },
        default_container: if dc.default_container.is_empty() {
            None
        } else {
            Some(dc.default_container.clone())
        },
        docker_command: dc.docker_command.clone(),
        devcontainer_command: dc.devcontainer_command.clone(),
        default_shell: if dc.default_shell.is_empty() {
            None
        } else {
            Some(dc.default_shell.clone())
        },
        override_user: if dc.override_user.is_empty() {
            None
        } else {
            Some(dc.override_user.clone())
        },
        poll_interval_secs: dc.poll_interval_secs.parse().unwrap_or(10),
        auto_discover: dc.auto_discover,
    }
}

// --- end weezterm remote features ---

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ssh_domain_config_default() {
        let config = SshDomainConfig::default();
        assert_eq!(config.name, "");
        assert_eq!(config.remote_address, "");
        assert_eq!(config.username, "");
        assert_eq!(config.multiplexing, "None");
        assert_eq!(config.ssh_backend, "LibSsh");
        assert!(!config.no_agent_auth);
        assert!(!config.connect_automatically);
    }

    #[test]
    fn test_domain_field_value_read() {
        let config = SshDomainConfig {
            name: "test-host".to_string(),
            remote_address: "10.0.0.1:22".to_string(),
            username: "deploy".to_string(),
            multiplexing: "WezTerm".to_string(),
            ssh_backend: "Ssh2".to_string(),
            no_agent_auth: true,
            connect_automatically: false,
        };
        assert_eq!(domain_field_value(&config, "remote_address"), "10.0.0.1:22");
        assert_eq!(domain_field_value(&config, "username"), "deploy");
        assert_eq!(domain_field_value(&config, "multiplexing"), "WezTerm");
        assert_eq!(domain_field_value(&config, "ssh_backend"), "Ssh2");
        assert_eq!(domain_field_value(&config, "no_agent_auth"), "On");
        assert_eq!(domain_field_value(&config, "connect_automatically"), "Off");
        assert_eq!(domain_field_value(&config, "unknown"), "");
    }

    #[test]
    fn test_domain_field_value_empty_username() {
        let config = SshDomainConfig::default();
        assert_eq!(domain_field_value(&config, "username"), "");
    }

    #[test]
    fn test_set_domain_field() {
        let mut config = SshDomainConfig::default();
        set_domain_field(&mut config, "remote_address", "myhost:22");
        assert_eq!(config.remote_address, "myhost:22");

        set_domain_field(&mut config, "username", "root");
        assert_eq!(config.username, "root");

        set_domain_field(&mut config, "multiplexing", "WezTerm");
        assert_eq!(config.multiplexing, "WezTerm");

        set_domain_field(&mut config, "ssh_backend", "Ssh2");
        assert_eq!(config.ssh_backend, "Ssh2");

        set_domain_field(&mut config, "no_agent_auth", "On");
        assert!(config.no_agent_auth);

        set_domain_field(&mut config, "no_agent_auth", "Off");
        assert!(!config.no_agent_auth);

        set_domain_field(&mut config, "connect_automatically", "true");
        assert!(config.connect_automatically);

        // Unknown field is a no-op
        set_domain_field(&mut config, "nonexistent", "value");
    }

    #[test]
    fn test_domain_field_defs_completeness() {
        let defs = domain_field_defs();
        let expected_keys = vec![
            "remote_address",
            "username",
            "multiplexing",
            "ssh_backend",
            "no_agent_auth",
            "connect_automatically",
        ];
        let actual_keys: Vec<&str> = defs.iter().map(|(k, _, _, _)| *k).collect();
        assert_eq!(actual_keys, expected_keys);
    }

    #[test]
    fn test_to_config_ssh_domain() {
        let dom = SshDomainConfig {
            name: "myhost".to_string(),
            remote_address: "myhost:22".to_string(),
            username: "root".to_string(),
            multiplexing: "WezTerm".to_string(),
            ssh_backend: "Ssh2".to_string(),
            no_agent_auth: true,
            connect_automatically: true,
        };
        let ssh_dom = to_config_ssh_domain(&dom);
        assert_eq!(ssh_dom.name, "myhost");
        assert_eq!(ssh_dom.remote_address, "myhost:22");
        assert_eq!(ssh_dom.username, Some("root".to_string()));
        assert!(ssh_dom.no_agent_auth);
        assert!(ssh_dom.connect_automatically);
    }

    #[test]
    fn test_to_config_ssh_domain_empty_username() {
        let dom = SshDomainConfig {
            username: String::new(),
            ..Default::default()
        };
        let ssh_dom = to_config_ssh_domain(&dom);
        assert_eq!(ssh_dom.username, None);
    }

    #[test]
    fn test_to_config_ssh_domain_multiplexing_none() {
        let dom = SshDomainConfig {
            multiplexing: "None".to_string(),
            ..Default::default()
        };
        let ssh_dom = to_config_ssh_domain(&dom);
        assert_eq!(ssh_dom.multiplexing, config::SshMultiplexing::None);
    }

    #[test]
    fn test_domain_entry_source_equality() {
        assert_eq!(DomainSource::Lua, DomainSource::Lua);
        assert_eq!(DomainSource::Overlay, DomainSource::Overlay);
        assert_ne!(DomainSource::Lua, DomainSource::Overlay);
    }
}
