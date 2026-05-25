// --- weezterm remote features ---
//! Cross-platform shims for Windows render-mode diagnostics.
//!
//! On Windows, this module re-exports the implementations from
//! `crate::os::windows`. On other platforms, it provides no-op stubs that
//! return sensible defaults so cross-platform call sites can use the same
//! API without `#[cfg(windows)]` gates.
//!
//! Phase 0 of `docs/windows-rendering-design.md` — these helpers are
//! consumed by the startup diagnostics log line in `wezterm-gui` and by
//! `RenderMode::auto_select()` in later phases.

#[cfg(windows)]
pub use crate::os::windows::{
    enumerate_dxgi_adapters, only_virtual_gpus_available, windows_build_number, DxgiAdapterInfo,
};

#[cfg(not(windows))]
mod stubs {
    /// A description of a single DXGI adapter, suitable for logging.
    #[derive(Debug, Clone)]
    pub struct DxgiAdapterInfo {
        pub description: String,
        pub vendor_id: u32,
        pub device_id: u32,
        pub is_software: bool,
    }

    pub fn windows_build_number() -> u32 {
        0
    }

    pub fn enumerate_dxgi_adapters() -> Vec<DxgiAdapterInfo> {
        Vec::new()
    }

    pub fn only_virtual_gpus_available() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub use self::stubs::*;

/// Reads `WEEZTERM_RENDER_MODE` if set. Returns the raw lowercase string
/// (one of `"auto"`, `"wgpu_dcomp"`, `"wgpu_classic"`, `"software_rdp"`),
/// or `None` if the variable is unset or holds an unrecognized value.
///
/// Logs a `warn!` if the value is unrecognized so users notice typos.
///
/// Phase 0 only parses; Phase 1 wires the override into mode selection.
pub fn render_mode_override() -> Option<String> {
    let raw = std::env::var("WEEZTERM_RENDER_MODE").ok()?;
    let lower = raw.to_lowercase();
    match lower.as_str() {
        "auto" | "wgpu_dcomp" | "wgpu_classic" | "software_rdp" => Some(lower),
        _ => {
            log::warn!("WEEZTERM_RENDER_MODE='{}' not recognized; ignoring", raw);
            None
        }
    }
}
// --- end weezterm remote features ---
