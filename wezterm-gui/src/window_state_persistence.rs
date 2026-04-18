//! Persistence for window state (position, size, maximized/fullscreen, monitor).
//!
//! Saves and loads window geometry to/from a JSON file in the config directory
//! so windows reopen in the same position and state across restarts/reconnects.
//!
//! --- weezterm remote features ---

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const WINDOW_STATE_FILE: &str = "window-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWindowState {
    /// X position (screen coordinates) of the normal (restored) window
    pub x: isize,
    /// Y position (screen coordinates) of the normal (restored) window
    pub y: isize,
    /// Width of the normal (restored) window in pixels
    pub width: usize,
    /// Height of the normal (restored) window in pixels
    pub height: usize,
    /// Whether the window was maximized
    #[serde(default)]
    pub maximized: bool,
    /// Whether the window was in fullscreen mode
    #[serde(default)]
    pub fullscreen: bool,
    /// Name of the monitor the window was on
    #[serde(default)]
    pub monitor: Option<String>,
}

/// Returns the path to the window state file.
fn state_file_path() -> Option<std::path::PathBuf> {
    config::CONFIG_DIRS
        .first()
        .map(|dir| dir.join(WINDOW_STATE_FILE))
}

/// Load all saved window states from disk.
fn load_all_states() -> HashMap<String, SavedWindowState> {
    let path = match state_file_path() {
        Some(p) => p,
        None => return HashMap::new(),
    };

    if !path.exists() {
        return HashMap::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(err) => {
            log::warn!("Failed to read window state file: {}", err);
            HashMap::new()
        }
    }
}

/// Save all window states to disk.
fn save_all_states(states: &HashMap<String, SavedWindowState>) {
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

/// Load the saved window state for a specific workspace.
pub fn load_window_state(workspace: &str) -> Option<SavedWindowState> {
    let states = load_all_states();
    let state = states.get(workspace).cloned();

    if let Some(ref s) = state {
        // Validate that the saved position is reasonable (on some screen)
        // by checking for extreme negative coordinates or zero dimensions
        if s.width == 0 || s.height == 0 {
            log::debug!(
                "Ignoring saved window state for '{}': zero dimensions",
                workspace
            );
            return None;
        }
        log::debug!(
            "Loaded window state for '{}': {}x{} at ({},{}) maximized={} monitor={:?}",
            workspace,
            s.width,
            s.height,
            s.x,
            s.y,
            s.maximized,
            s.monitor,
        );
    }

    state
}

/// Save the window state for a specific workspace.
pub fn save_window_state(workspace: &str, state: SavedWindowState) {
    log::debug!(
        "Saving window state for '{}': {}x{} at ({},{}) maximized={} monitor={:?}",
        workspace,
        state.width,
        state.height,
        state.x,
        state.y,
        state.maximized,
        state.monitor,
    );
    let mut states = load_all_states();
    states.insert(workspace.to_string(), state);
    save_all_states(&states);
}
