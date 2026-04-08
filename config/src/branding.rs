// --- weezterm remote features ---
//! Central branding constants and helpers for the Weezterm fork.
//!
//! All fork-specific naming lives here so that upstream file modifications
//! are minimal (just one-line sentinel-wrapped references to this module).

use portable_pty::CommandBuilder;
use std::ffi::OsString;
use std::path::PathBuf;

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
// Remote browser helper — shared between mux-server and non-mux SSH paths
// ---------------------------------------------------------------------------

/// Path to the browser helper script, relative to $HOME.
pub const REMOTE_BROWSER_RELATIVE: &str = ".weezterm/browser.sh";

/// Resolve the absolute path to the browser helper script (`$HOME/.weezterm/browser.sh`).
pub fn remote_browser_path() -> PathBuf {
    crate::HOME_DIR.join(REMOTE_BROWSER_RELATIVE)
}

/// The shell script content for the remote browser helper.
/// Sends URLs back to the client via OSC 7457 so they open in the local browser.
#[cfg(unix)]
const BROWSER_HELPER_SCRIPT: &str = "\
#!/bin/sh\nprintf '\\033]7457;open-url;%s\\033\\\\' \"$1\" >/dev/tty\n";

/// Write the browser helper script to `path`, creating parent directories
/// and setting the executable bit.  This is the single source of truth for
/// the script content.
#[cfg(unix)]
pub fn write_browser_helper_script(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, BROWSER_HELPER_SCRIPT)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Ensure the browser helper script exists at `$HOME/.weezterm/browser.sh`.
/// Returns the absolute path.  Idempotent — only writes if the file is
/// missing.  Returns `Err` only on I/O failure during write.
#[cfg(unix)]
pub fn ensure_browser_helper() -> anyhow::Result<String> {
    let path = remote_browser_path();
    if !path.exists() {
        write_browser_helper_script(&path)
            .map_err(|e| anyhow::anyhow!("failed to write browser helper {}: {}", path.display(), e))?;
    }
    Ok(path.to_string_lossy().into_owned())
}

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
