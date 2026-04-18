// --- weezterm remote features ---
use serde::{Deserialize, Serialize};
use wezterm_dynamic::{FromDynamic, ToDynamic};

/// Per-monitor configuration overrides.
///
/// When the terminal window moves onto a monitor whose name matches
/// `monitor`, the specified overrides (e.g. `color_scheme`) are applied.
/// When the window leaves that monitor, the overrides are removed and
/// the user's base configuration is restored.
///
/// Monitor names are the same strings visible in `dpi_by_screen`:
/// - Windows: friendly display name from QueryDisplayConfig (e.g. "DELL U2720Q")
/// - macOS:  NSScreen localizedName (e.g. "Color LCD")
/// - X11:   RANDR output name (e.g. "DP-1")
/// - Wayland: wlr_output name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromDynamic, ToDynamic)]
pub struct MonitorOverride {
    /// The monitor name to match.
    pub monitor: String,

    /// If set, use this color scheme when the window is on this monitor.
    #[dynamic(default)]
    pub color_scheme: Option<String>,
    // Future fields: font_size, opacity, background, window_background_opacity, etc.
}
// --- end weezterm remote features ---
