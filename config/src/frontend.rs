use luahelper::impl_lua_conversion_dynamic;
use wezterm_dynamic::{FromDynamic, ToDynamic};

// --- weezterm remote features ---
// Phase 4d: default to `Auto` on Windows. The platform-conditional
// default lets WeezTerm pick `SoftwareRdp` automatically on RDP boxes
// where neither real OpenGL nor a usable wgpu adapter is available.
// On non-Windows targets we keep the upstream default (`OpenGL`) so a
// merge from upstream/main does not change behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
#[allow(deprecated)] // OpenGL variant kept for back-compat; derive references all variants by name.
pub enum FrontEndSelection {
    #[deprecated(
        since = "WeezTerm-rendering-overhaul",
        note = "OpenGL/glium is incompatible with DWM (no flip-model swap chain, classic stretch \
                on resize) and will be removed once Mode C (SoftwareRdp) has shipped for one \
                full release. Migrate to `Auto` (the new default on Windows). \
                See docs/windows-rendering-design.md for the full design."
    )]
    OpenGL,
    WebGpu,
    Software,
    Auto,
    WebGpuHwnd,
}

#[allow(deprecated)] // Reference to OpenGL variant in the non-Windows back-compat default.
impl Default for FrontEndSelection {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Auto
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::OpenGL
        }
    }
}
// --- end weezterm remote features ---

/// Corresponds to <https://docs.rs/wgpu/latest/wgpu/struct.AdapterInfo.html>
#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct GpuInfo {
    pub name: String,
    pub device_type: String,
    pub backend: String,
    pub driver: Option<String>,
    pub driver_info: Option<String>,
    pub vendor: Option<u32>,
    pub device: Option<u32>,
}
impl_lua_conversion_dynamic!(GpuInfo);

impl ToString for GpuInfo {
    fn to_string(&self) -> String {
        let mut result = format!(
            "name={}, device_type={}, backend={}",
            self.name, self.device_type, self.backend
        );
        if let Some(driver) = &self.driver {
            result.push_str(&format!(", driver={driver}"));
        }
        if let Some(driver_info) = &self.driver_info {
            result.push_str(&format!(", driver_info={driver_info}"));
        }
        if let Some(vendor) = &self.vendor {
            result.push_str(&format!(", vendor={vendor}"));
        }
        if let Some(device) = &self.device {
            result.push_str(&format!(", device={device}"));
        }
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum WebGpuPowerPreference {
    LowPower,
    HighPerformance,
}

impl Default for WebGpuPowerPreference {
    fn default() -> Self {
        Self::LowPower
    }
}
