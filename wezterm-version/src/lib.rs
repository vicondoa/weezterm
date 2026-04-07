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
/// The version is something like "20240203-110809-abc12345.weez.1"
/// and the release tag is "v20240203-110809-abc12345.weez.1".
pub fn release_tag() -> String {
    format!("v{}", wezterm_version())
}
// --- end weezterm remote features ---
