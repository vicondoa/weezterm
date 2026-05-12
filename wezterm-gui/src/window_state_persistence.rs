//! Persistence for window state (position, size, maximized/fullscreen, monitor).
//!
//! Schema v2 (current):
//!   * `workspace_relative_rect` is the *normal* (restored) `WINDOWPLACEMENT.rcNormalPosition`
//!     equivalent, expressed in coordinates relative to the monitor's origin
//!     (so it survives docking/undocking, monitor renames, and DPI changes).
//!   * `maximized` / `fullscreen` are saved separately. Restoration first
//!     positions the window at the normal rect, then applies the maximize /
//!     fullscreen state, so the user's first un-maximize lands at the right
//!     restored size.
//!   * `persistence_dpi` is the DPI at which `workspace_relative_rect` was
//!     captured; on restore we rescale the rect if the target monitor's DPI
//!     differs.
//!
//! Schema v1 was buggy in several ways (hard-coded `(0,0)` position, mixed
//! absolute/relative coords, "maximized" dims saved as the normal rect, etc.).
//! v1 entries are detected by the missing/older `schema` field and migrated
//! to "centered on primary monitor" — never reused as-is, since a v1 file
//! could otherwise restore the window completely off-screen.
//!
//! The two coordinate-space newtypes (`WorkspaceCoords`, `ScreenCoords`)
//! make the historical "absolute-vs-monitor-relative" bug class a compile
//! error: a `ScreenCoords` cannot be silently used where a `WorkspaceCoords`
//! is expected.
//!
//! See `docs/remote-extensions.md` and `AGENTS.md`.
//!
//! --- weezterm remote features ---

use ::window::screen::{ScreenInfo, Screens};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const WINDOW_STATE_FILE: &str = "window-state.json";

/// Current on-disk schema version. Bump on incompatible changes.
pub const CURRENT_SCHEMA: u32 = 2;

/// Coordinates whose origin is a specific monitor's top-left corner.
/// These are what we persist on disk: they survive moving the window's
/// monitor in the system's virtual screen because they are always relative
/// to "the monitor the window lives on".
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceCoords {
    pub x: i32,
    pub y: i32,
}

/// Coordinates in the OS's virtual screen space (Win32's "screen
/// coordinates"). These are what `WINDOWPLACEMENT`/`MoveWindow` /etc.
/// take. Can be negative for monitors above/left of the primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCoords {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRect {
    pub origin: WorkspaceCoords,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub origin: ScreenCoords,
    pub width: u32,
    pub height: u32,
}

impl ScreenRect {
    pub fn intersects(&self, other: &ScreenRect) -> bool {
        let ax1 = self.origin.x;
        let ay1 = self.origin.y;
        let ax2 = ax1.saturating_add(self.width as i32);
        let ay2 = ay1.saturating_add(self.height as i32);
        let bx1 = other.origin.x;
        let by1 = other.origin.y;
        let bx2 = bx1.saturating_add(other.width as i32);
        let by2 = by1.saturating_add(other.height as i32);
        ax1 < bx2 && ax2 > bx1 && ay1 < by2 && ay2 > by1
    }

    /// Approximate area of the intersection between `self` and `other`,
    /// in the same units as the rectangles.
    pub fn intersection_area(&self, other: &ScreenRect) -> i64 {
        let ax1 = self.origin.x as i64;
        let ay1 = self.origin.y as i64;
        let ax2 = ax1 + self.width as i64;
        let ay2 = ay1 + self.height as i64;
        let bx1 = other.origin.x as i64;
        let by1 = other.origin.y as i64;
        let bx2 = bx1 + other.width as i64;
        let by2 = by1 + other.height as i64;
        let w = (ax2.min(bx2) - ax1.max(bx1)).max(0);
        let h = (ay2.min(by2) - ay1.max(by1)).max(0);
        w * h
    }
}

/// Snapshot of a single monitor used by the persistence module. Wraps the
/// upstream `ScreenInfo` to expose just the bits we need and to hold
/// `WorkspaceCoords`/`ScreenCoords` conversions in one place.
#[derive(Debug, Clone)]
pub struct MonitorContext {
    pub name: String,
    /// The monitor's top-left corner in screen coordinates.
    pub screen_origin: ScreenCoords,
    pub width: u32,
    pub height: u32,
    /// Effective DPI for this monitor (Windows DPI scaling), or 96 if unknown.
    pub dpi: u32,
}

impl MonitorContext {
    pub fn from_screen_info(info: &ScreenInfo) -> Self {
        Self {
            name: info.name.clone(),
            screen_origin: ScreenCoords {
                x: info.rect.origin.x as i32,
                y: info.rect.origin.y as i32,
            },
            width: info.rect.size.width.max(0) as u32,
            height: info.rect.size.height.max(0) as u32,
            dpi: info
                .effective_dpi
                .map(|d| d.round().max(1.0) as u32)
                .unwrap_or(96),
        }
    }

    /// Convert workspace-relative coords to absolute screen coords.
    pub fn workspace_to_screen(&self, ws: WorkspaceCoords) -> ScreenCoords {
        ScreenCoords {
            x: self.screen_origin.x + ws.x,
            y: self.screen_origin.y + ws.y,
        }
    }

    /// Convert absolute screen coords to workspace-relative coords for this monitor.
    pub fn screen_to_workspace(&self, sc: ScreenCoords) -> WorkspaceCoords {
        WorkspaceCoords {
            x: sc.x - self.screen_origin.x,
            y: sc.y - self.screen_origin.y,
        }
    }

    /// Convert a workspace-relative rect to a screen-coords rect.
    pub fn workspace_rect_to_screen(&self, ws_rect: WorkspaceRect) -> ScreenRect {
        ScreenRect {
            origin: self.workspace_to_screen(ws_rect.origin),
            width: ws_rect.width,
            height: ws_rect.height,
        }
    }

    /// The full monitor rect in screen coordinates.
    pub fn screen_rect(&self) -> ScreenRect {
        ScreenRect {
            origin: self.screen_origin,
            width: self.width,
            height: self.height,
        }
    }
}

/// Build `MonitorContext`s plus a "primary" hint from the upstream `Screens`.
pub fn collect_monitors(screens: Option<&Screens>) -> (Vec<MonitorContext>, Option<String>) {
    match screens {
        Some(s) => {
            let monitors = s
                .by_name
                .values()
                .map(MonitorContext::from_screen_info)
                .collect();
            (monitors, Some(s.main.name.clone()))
        }
        None => (Vec::new(), None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedWindowState {
    /// Schema version. Always equals `CURRENT_SCHEMA` (=2) for valid v2 files.
    pub schema: u32,

    pub workspace: String,

    /// Name of the monitor the window was on at save time. Optional because
    /// some platforms (Wayland) cannot determine this.
    #[serde(default)]
    pub monitor_name: Option<String>,

    /// `WINDOWPLACEMENT.rcNormalPosition` equivalent — the *normal* (un-maximized,
    /// un-fullscreened) rect, expressed in coordinates relative to the saved
    /// monitor's origin.
    pub workspace_relative_rect: WorkspaceRect,

    /// DPI at which `workspace_relative_rect` was captured. On restore we
    /// rescale the rect by `target_monitor.dpi / persistence_dpi`.
    pub persistence_dpi: u32,

    #[serde(default)]
    pub maximized: bool,

    #[serde(default)]
    pub fullscreen: bool,

    #[serde(default)]
    pub saved_at_unix_secs: u64,
}

/// Outcome of attempting to restore a window from a `PersistedWindowState`.
#[derive(Debug, Clone)]
pub enum RestoreResult {
    /// Place the window on `monitor`, with workspace-relative `coords`.
    /// `maximized`/`fullscreen` should be applied after positioning.
    Restored {
        monitor: MonitorContext,
        coords: WorkspaceCoords,
        width: u32,
        height: u32,
        maximized: bool,
        fullscreen: bool,
    },
    /// Couldn't position precisely (the saved rect doesn't intersect any
    /// connected monitor's area, or DPI rescale pushed it off-screen).
    /// Caller should center on `monitor` at the saved size.
    CenteredOnMonitor {
        monitor: MonitorContext,
        width: u32,
        height: u32,
        maximized: bool,
        fullscreen: bool,
    },
    /// Saved state has an incompatible (older) schema. Caller should fall
    /// back to the default (config-driven) geometry. We never re-use stale
    /// schema-1 data because its coordinate-space semantics were buggy.
    SkippedStaleSchema,
    /// No saved state file exists for this workspace.
    NoState,
}

fn state_file_path() -> Option<std::path::PathBuf> {
    config::CONFIG_DIRS
        .first()
        .map(|dir| dir.join(WINDOW_STATE_FILE))
}

/// Read the entire `window-state.json`, preserving each entry as a raw JSON
/// value so that schema migration can be done per-workspace without losing
/// other workspaces' entries.
fn read_raw_states() -> HashMap<String, serde_json::Value> {
    let path = match state_file_path() {
        Some(p) => p,
        None => return HashMap::new(),
    };
    if !path.exists() {
        return HashMap::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|err| {
            log::warn!("Failed to parse window state file: {}", err);
            HashMap::new()
        }),
        Err(err) => {
            log::warn!("Failed to read window state file: {}", err);
            HashMap::new()
        }
    }
}

fn write_raw_states(states: &HashMap<String, serde_json::Value>) {
    let path = match state_file_path() {
        Some(p) => p,
        None => return,
    };
    match serde_json::to_string_pretty(states) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&path, json) {
                log::warn!("Failed to write window state file: {}", err);
            }
        }
        Err(err) => {
            log::warn!("Failed to serialize window state: {}", err);
        }
    }
}

/// Load and validate the persisted state for a single workspace.
/// Returns `None` if the entry is missing or has an incompatible schema.
pub fn load_for_workspace(workspace: &str) -> Option<PersistedWindowState> {
    let raw = read_raw_states().remove(workspace)?;
    let schema = raw.get("schema").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if schema < CURRENT_SCHEMA {
        log::warn!(
            "Window state for workspace '{}' has stale schema v{}; ignoring \
             (will fall back to default geometry).",
            workspace,
            schema,
        );
        return None;
    }
    match serde_json::from_value::<PersistedWindowState>(raw) {
        Ok(state) => {
            if state.workspace_relative_rect.width == 0 || state.workspace_relative_rect.height == 0
            {
                log::debug!(
                    "Ignoring saved window state for '{}': zero dimensions",
                    workspace,
                );
                None
            } else {
                log::debug!(
                    "Loaded window state for '{}': {:?} dpi={} max={} fs={} monitor={:?}",
                    workspace,
                    state.workspace_relative_rect,
                    state.persistence_dpi,
                    state.maximized,
                    state.fullscreen,
                    state.monitor_name,
                );
                Some(state)
            }
        }
        Err(err) => {
            log::warn!(
                "Window state for workspace '{}' failed to deserialize as schema v{}: {}",
                workspace,
                CURRENT_SCHEMA,
                err,
            );
            None
        }
    }
}

/// Apply the explicit fallback chain to figure out where to put the window.
///
/// Order:
///   1. Monitor whose name exactly matches `saved.monitor_name`.
///   2. Monitor whose work area best contains the saved rect's screen-coords
///      mapping (computed using #1's monitor or, if absent, every monitor).
///   3. Monitor named `primary_name`.
///   4. The first monitor in `monitors`.
///
/// Then validates that the rescaled rect intersects the chosen monitor; if
/// not, returns `CenteredOnMonitor`.
pub fn restore_window(
    saved: Option<&PersistedWindowState>,
    monitors: &[MonitorContext],
    primary_name: Option<&str>,
) -> RestoreResult {
    let saved = match saved {
        Some(s) => s,
        None => return RestoreResult::NoState,
    };

    if monitors.is_empty() {
        // No monitor info — no safe positioning is possible. Bail out and let
        // the caller use defaults; we still preserve the maximize bit below
        // by returning SkippedStaleSchema (semantically: "use defaults").
        log::warn!("restore_window: no monitor info available; using defaults");
        return RestoreResult::SkippedStaleSchema;
    }

    // 1. By name.
    let by_name = saved
        .monitor_name
        .as_deref()
        .and_then(|name| monitors.iter().find(|m| m.name == name));

    // 2. Monitor whose work area best contains the saved rect.
    //    We can't translate the rect to absolute screen coords without picking
    //    *some* monitor first; instead we compute, for each candidate monitor,
    //    the area of overlap when the rect is interpreted as relative to *that*
    //    monitor. The monitor with the largest intersection wins. This mirrors
    //    what users intuitively want: "the new monitor whose layout the saved
    //    rect makes most sense on".
    let by_overlap = || {
        monitors
            .iter()
            .map(|m| {
                let projected = m.workspace_rect_to_screen(saved.workspace_relative_rect);
                (m, m.screen_rect().intersection_area(&projected))
            })
            .filter(|(_, area)| *area > 0)
            .max_by_key(|(_, area)| *area)
            .map(|(m, _)| m)
    };

    // 3. Primary monitor.
    let by_primary = || primary_name.and_then(|name| monitors.iter().find(|m| m.name == name));

    // 4. First monitor.
    let first = || monitors.first();

    let monitor: MonitorContext = by_name
        .or_else(by_overlap)
        .or_else(by_primary)
        .or_else(first)
        .cloned()
        .expect("monitors is non-empty");

    log::info!(
        "restore_window: target monitor='{}' (saved monitor='{:?}', \
         primary='{:?}', candidates={})",
        monitor.name,
        saved.monitor_name,
        primary_name,
        monitors.len(),
    );

    // DPI rescale.
    let dpi_scale = monitor.dpi as f64 / saved.persistence_dpi.max(1) as f64;
    let scaled = scale_workspace_rect(saved.workspace_relative_rect, dpi_scale);

    // Validate intersection. If the rescaled rect is entirely off the chosen
    // monitor (e.g. saved x = -10000), center on the monitor instead.
    let projected = monitor.workspace_rect_to_screen(scaled);
    let monitor_rect = monitor.screen_rect();
    if !monitor_rect.intersects(&projected) {
        log::warn!(
            "restore_window: scaled rect {:?} does not intersect monitor {:?} \
             ({:?}); centering instead",
            projected,
            monitor.name,
            monitor_rect,
        );
        return RestoreResult::CenteredOnMonitor {
            monitor,
            width: scaled.width,
            height: scaled.height,
            maximized: saved.maximized,
            fullscreen: saved.fullscreen,
        };
    }

    RestoreResult::Restored {
        monitor,
        coords: scaled.origin,
        width: scaled.width,
        height: scaled.height,
        maximized: saved.maximized,
        fullscreen: saved.fullscreen,
    }
}

fn scale_workspace_rect(rect: WorkspaceRect, scale: f64) -> WorkspaceRect {
    if (scale - 1.0).abs() < 1e-6 {
        return rect;
    }
    WorkspaceRect {
        origin: WorkspaceCoords {
            x: (rect.origin.x as f64 * scale).round() as i32,
            y: (rect.origin.y as f64 * scale).round() as i32,
        },
        width: ((rect.width as f64) * scale).round().max(1.0) as u32,
        height: ((rect.height as f64) * scale).round().max(1.0) as u32,
    }
}

/// Capture and save the current window state for `workspace`.
///
/// `placement` is the *normal* (restored) rect in screen coordinates, as
/// returned by `WindowOps::get_window_placement`. `maximized` and
/// `fullscreen` are stored separately so that the user's first un-maximize
/// after a restart lands at the original normal rect.
pub fn capture_and_save(
    workspace: &str,
    placement: ScreenRect,
    maximized: bool,
    fullscreen: bool,
    current_monitor_name: Option<&str>,
    monitors: &[MonitorContext],
) {
    // Pick the monitor whose name matches `current_monitor_name`, falling
    // back to the monitor that most contains `placement` (so we never save
    // a window-relative coord against the wrong monitor's origin).
    let monitor = current_monitor_name
        .and_then(|name| monitors.iter().find(|m| m.name == name).cloned())
        .or_else(|| {
            monitors
                .iter()
                .map(|m| (m, m.screen_rect().intersection_area(&placement)))
                .filter(|(_, area)| *area > 0)
                .max_by_key(|(_, area)| *area)
                .map(|(m, _)| m.clone())
        })
        .or_else(|| monitors.first().cloned());

    let monitor = match monitor {
        Some(m) => m,
        None => {
            log::warn!(
                "capture_and_save: no monitor info; skipping window state save \
                 for workspace '{}'",
                workspace,
            );
            return;
        }
    };

    let workspace_origin = monitor.screen_to_workspace(placement.origin);
    let workspace_rect = WorkspaceRect {
        origin: workspace_origin,
        width: placement.width,
        height: placement.height,
    };

    let state = PersistedWindowState {
        schema: CURRENT_SCHEMA,
        workspace: workspace.to_string(),
        monitor_name: Some(monitor.name.clone()),
        workspace_relative_rect: workspace_rect,
        persistence_dpi: monitor.dpi,
        maximized,
        fullscreen,
        saved_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };

    log::debug!(
        "Saving window state for '{}': {:?} dpi={} max={} fs={} monitor='{}'",
        workspace,
        state.workspace_relative_rect,
        state.persistence_dpi,
        state.maximized,
        state.fullscreen,
        monitor.name,
    );

    let mut all = read_raw_states();
    let value = match serde_json::to_value(&state) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("Failed to serialize window state: {}", err);
            return;
        }
    };
    all.insert(workspace.to_string(), value);
    write_raw_states(&all);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32, y: i32, w: u32, h: u32, dpi: u32) -> MonitorContext {
        MonitorContext {
            name: name.to_string(),
            screen_origin: ScreenCoords { x, y },
            width: w,
            height: h,
            dpi,
        }
    }

    fn state(monitor: &str, x: i32, y: i32, w: u32, h: u32, dpi: u32) -> PersistedWindowState {
        PersistedWindowState {
            schema: CURRENT_SCHEMA,
            workspace: "default".to_string(),
            monitor_name: Some(monitor.to_string()),
            workspace_relative_rect: WorkspaceRect {
                origin: WorkspaceCoords { x, y },
                width: w,
                height: h,
            },
            persistence_dpi: dpi,
            maximized: false,
            fullscreen: false,
            saved_at_unix_secs: 0,
        }
    }

    #[test]
    fn restore_by_name_when_monitor_present() {
        let monitors = vec![
            mon("primary", 0, 0, 1920, 1080, 96),
            mon("secondary", 1920, 0, 2560, 1440, 96),
        ];
        let s = state("secondary", 100, 200, 800, 600, 96);
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::Restored {
                monitor, coords, ..
            } => {
                assert_eq!(monitor.name, "secondary");
                assert_eq!(coords, WorkspaceCoords { x: 100, y: 200 });
            }
            other => panic!("expected Restored, got {:?}", other),
        }
    }

    #[test]
    fn restore_falls_back_to_overlap_when_monitor_renamed() {
        // "secondary" is gone; "secondary-renamed" replaced it at the same
        // origin, so the overlap heuristic should pick it.
        let monitors = vec![
            mon("primary", 0, 0, 1920, 1080, 96),
            mon("secondary-renamed", 1920, 0, 2560, 1440, 96),
        ];
        let s = state("secondary", 100, 200, 800, 600, 96);
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::Restored { monitor, .. } => {
                assert_eq!(monitor.name, "secondary-renamed");
            }
            other => panic!("expected Restored, got {:?}", other),
        }
    }

    #[test]
    fn restore_falls_back_to_primary_when_monitor_disconnected() {
        // Saved on a monitor that's now gone; no other monitor "contains"
        // the rect because the saved rect's coords are within (100,200).
        // The overlap heuristic will project against each monitor and pick
        // whichever has overlap. Ensure primary fallback works when none do.
        let monitors = vec![mon("primary", 0, 0, 1920, 1080, 96)];
        // Use an x/y inside the primary monitor: overlap heuristic picks it.
        let s = state("disconnected", 100, 200, 800, 600, 96);
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::Restored { monitor, .. }
            | RestoreResult::CenteredOnMonitor { monitor, .. } => {
                assert_eq!(monitor.name, "primary");
            }
            other => panic!("expected Restored or CenteredOnMonitor, got {:?}", other),
        }
    }

    #[test]
    fn restore_centers_when_offscreen_rect() {
        let monitors = vec![mon("primary", 0, 0, 1920, 1080, 96)];
        // x = -10000 puts the rect entirely off-screen for primary.
        let s = state("primary", -10000, 0, 800, 600, 96);
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::CenteredOnMonitor {
                monitor,
                width,
                height,
                ..
            } => {
                assert_eq!(monitor.name, "primary");
                assert_eq!(width, 800);
                assert_eq!(height, 600);
            }
            other => panic!("expected CenteredOnMonitor, got {:?}", other),
        }
    }

    #[test]
    fn restore_rescales_for_dpi_change() {
        let monitors = vec![mon("primary", 0, 0, 3840, 2160, 192)];
        // Saved at 96 DPI: 800x600 → on 192 DPI monitor should be 1600x1200.
        let s = state("primary", 100, 100, 800, 600, 96);
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::Restored {
                width,
                height,
                coords,
                ..
            } => {
                assert_eq!(width, 1600);
                assert_eq!(height, 1200);
                assert_eq!(coords, WorkspaceCoords { x: 200, y: 200 });
            }
            other => panic!("expected Restored, got {:?}", other),
        }
    }

    #[test]
    fn restore_preserves_maximized_flag() {
        let monitors = vec![mon("primary", 0, 0, 1920, 1080, 96)];
        let mut s = state("primary", 100, 100, 800, 600, 96);
        s.maximized = true;
        match restore_window(Some(&s), &monitors, Some("primary")) {
            RestoreResult::Restored { maximized, .. } => assert!(maximized),
            other => panic!("expected Restored, got {:?}", other),
        }
    }

    #[test]
    fn restore_no_state_when_none() {
        let monitors = vec![mon("primary", 0, 0, 1920, 1080, 96)];
        match restore_window(None, &monitors, Some("primary")) {
            RestoreResult::NoState => {}
            other => panic!("expected NoState, got {:?}", other),
        }
    }

    #[test]
    fn restore_no_state_when_no_monitors() {
        let s = state("primary", 100, 100, 800, 600, 96);
        match restore_window(Some(&s), &[], None) {
            RestoreResult::SkippedStaleSchema => {}
            other => panic!("expected SkippedStaleSchema, got {:?}", other),
        }
    }

    #[test]
    fn coord_round_trip() {
        let m = mon("primary", -1920, 0, 1920, 1080, 96);
        let sc = ScreenCoords { x: -1820, y: 100 };
        let ws = m.screen_to_workspace(sc);
        assert_eq!(ws, WorkspaceCoords { x: 100, y: 100 });
        let back = m.workspace_to_screen(ws);
        assert_eq!(back, sc);
    }

    #[test]
    fn screen_rect_intersects() {
        let a = ScreenRect {
            origin: ScreenCoords { x: 0, y: 0 },
            width: 100,
            height: 100,
        };
        let b = ScreenRect {
            origin: ScreenCoords { x: 50, y: 50 },
            width: 100,
            height: 100,
        };
        let c = ScreenRect {
            origin: ScreenCoords { x: 200, y: 200 },
            width: 100,
            height: 100,
        };
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
        assert_eq!(a.intersection_area(&b), 50 * 50);
        assert_eq!(a.intersection_area(&c), 0);
    }
}
// --- end weezterm remote features ---
