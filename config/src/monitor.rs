// --- weezterm remote features ---
use serde::{Deserialize, Serialize};
use wezterm_dynamic::{FromDynamic, ToDynamic};

/// Per-monitor configuration overrides.
///
/// When the terminal window moves onto a matching monitor, the specified
/// overrides (e.g. `color_scheme`) are applied. When the window leaves
/// that monitor, the overrides are removed and the user's base
/// configuration is restored.
///
/// Matching can be done by **name** or by **position**. Set exactly one:
///
/// - `monitor`: match by display name (same strings visible in
///   `dpi_by_screen`). Names vary by platform and may change across
///   remote-desktop sessions.
/// - `position`: match by grid position. Monitors are sorted into a
///   row/column grid based on their screen coordinates. Values:
///   - 2×2: `"top-left"`, `"top-right"`, `"bottom-left"`, `"bottom-right"`
///   - side-by-side: `"left"`, `"right"` (+ `"center"` for 3)
///   - stacked: `"top"`, `"bottom"` (+ `"middle"` for 3)
///   - arbitrary: `"row0-col0"`, `"row1-col2"`, etc.
///
/// If both are set, both must match (AND).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromDynamic, ToDynamic)]
pub struct MonitorOverride {
    /// The monitor name to match (optional).
    #[dynamic(default)]
    pub monitor: Option<String>,

    /// The grid position to match (optional).
    /// Computed from the physical layout of connected monitors.
    #[dynamic(default)]
    pub position: Option<String>,

    /// If set, use this color scheme when the window is on this monitor.
    #[dynamic(default)]
    pub color_scheme: Option<String>,
    // Future fields: font_size, opacity, background, window_background_opacity, etc.
}
// --- end weezterm remote features ---
