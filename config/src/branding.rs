// --- weezterm remote features ---
//! Central branding constants and helpers for the Weezterm fork.
//!
//! All fork-specific naming lives here so that upstream file modifications
//! are minimal (just one-line sentinel-wrapped references to this module).

use portable_pty::CommandBuilder;
use std::ffi::OsString;

// ---------------------------------------------------------------------------
// App identity
// ---------------------------------------------------------------------------

/// CLI / binary display name (lowercase)
pub const APP_NAME: &str = "weezterm";

/// Human-readable product name (title case)
pub const APP_NAME_DISPLAY: &str = "WeezTerm";

// ---------------------------------------------------------------------------
// Binary names (platform-appropriate)
// ---------------------------------------------------------------------------

pub const GUI_BIN: &str = if cfg!(windows) {
    "weezterm-gui.exe"
} else {
    "weezterm-gui"
};

pub const MUX_SERVER_BIN: &str = if cfg!(windows) {
    "weezterm-mux-server.exe"
} else {
    "weezterm-mux-server"
};

// ---------------------------------------------------------------------------
// Remote browser helper path (fork-only feature)
// ---------------------------------------------------------------------------

pub const REMOTE_BROWSER_PATH: &str = "/tmp/.weezterm-browser";

// ---------------------------------------------------------------------------
// Recording temp-file prefix
// ---------------------------------------------------------------------------

pub const RECORDING_PREFIX: &str = "weezterm-recording-";

// ---------------------------------------------------------------------------
// Environment variable helpers
//
// Weezterm sets both WEEZTERM_<X> and WEZTERM_<X> for backwards compat.
// When reading, it checks WEEZTERM_<X> first, falling back to WEZTERM_<X>.
// ---------------------------------------------------------------------------

/// Set a `WEEZTERM_<suffix>` and `WEZTERM_<suffix>` env var on a pty CommandBuilder.
pub fn set_env_with_compat(cmd: &mut CommandBuilder, suffix: &str, value: &str) {
    cmd.env(format!("WEEZTERM_{suffix}"), value);
    cmd.env(format!("WEZTERM_{suffix}"), value);
}

/// Read `WEEZTERM_<suffix>`, falling back to `WEZTERM_<suffix>`.
pub fn get_env_with_compat(suffix: &str) -> Option<OsString> {
    std::env::var_os(format!("WEEZTERM_{suffix}"))
        .or_else(|| std::env::var_os(format!("WEZTERM_{suffix}")))
}

/// Set env var on the current process (both names).
pub fn set_current_env_with_compat(suffix: &str, value: &str) {
    std::env::set_var(format!("WEEZTERM_{suffix}"), value);
    std::env::set_var(format!("WEZTERM_{suffix}"), value);
}

/// Remove env var from the current process (both names).
pub fn remove_current_env_with_compat(suffix: &str) {
    std::env::remove_var(format!("WEEZTERM_{suffix}"));
    std::env::remove_var(format!("WEZTERM_{suffix}"));
}
// --- end weezterm remote features ---
