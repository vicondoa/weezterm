pub fn wezterm_version() -> &'static str {
    // See build.rs
    env!("WEZTERM_CI_TAG")
}

pub fn wezterm_target_triple() -> &'static str {
    // See build.rs
    env!("WEZTERM_TARGET_TRIPLE")
}

// --- weezterm remote features ---
/// Derive the GitHub release tag from the version string.
/// For release builds this is "v0.2.0"; for dev builds "v0.2.0-dev.20240203.abc12345".
pub fn release_tag() -> String {
    format!("v{}", wezterm_version())
}
// --- end weezterm remote features ---
