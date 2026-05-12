// --- weezterm remote features ---
//! Render mode selection for the WeezTerm window stack.
//!
//! See `docs/windows-rendering-design.md` §4.

use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// wgpu DX12 + DComp + premultiplied alpha + waitable. Modern Win10/11 + GPU.
    WgpuDComp,
    /// wgpu DX12 without DComp. Win10 < 19041 or driver fallback.
    WgpuClassic,
    /// CPU rasteriser + WARP + Present1 with dirty rects. RDP / virtual GPU.
    SoftwareRdp,
}

impl RenderMode {
    /// Auto-select based on environment.
    /// On non-Windows, returns WgpuClassic (the existing wgpu path).
    pub fn auto_select() -> Self {
        #[cfg(windows)]
        {
            if crate::os::windows::is_running_in_rdp_session()
                || crate::os::windows::only_virtual_gpus_available()
            {
                return Self::SoftwareRdp;
            }
            if crate::os::windows::windows_build_number() < 19041 {
                return Self::WgpuClassic;
            }
            Self::WgpuDComp
        }
        #[cfg(not(windows))]
        {
            Self::WgpuClassic
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WgpuDComp => "wgpu_dcomp",
            Self::WgpuClassic => "wgpu_classic",
            Self::SoftwareRdp => "software_rdp",
        }
    }
}

impl FromStr for RenderMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "wgpu_dcomp" => Ok(Self::WgpuDComp),
            "wgpu_classic" => Ok(Self::WgpuClassic),
            "software_rdp" => Ok(Self::SoftwareRdp),
            other => Err(format!("unknown RenderMode: {other}")),
        }
    }
}

/// Resolves the render mode honouring `WEEZTERM_RENDER_MODE` if set and valid;
/// falls back to `auto_select()`.
pub fn resolve() -> RenderMode {
    if let Some(raw) = crate::diagnostics::render_mode_override() {
        if raw == "auto" {
            return RenderMode::auto_select();
        }
        if let Ok(mode) = raw.parse() {
            return mode;
        }
        // diagnostics::render_mode_override() already logged a warning; fall through.
    }
    RenderMode::auto_select()
}

/// Returns `true` when the resolved render mode is Mode A (`WgpuDComp`).
/// Used by the Windows back-end to gate legacy DWM blur / accent paths
/// that are incompatible with a DirectComposition-backed swapchain.
pub fn is_dcomp() -> bool {
    resolve() == RenderMode::WgpuDComp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip() {
        for m in [
            RenderMode::WgpuDComp,
            RenderMode::WgpuClassic,
            RenderMode::SoftwareRdp,
        ] {
            assert_eq!(m.as_str().parse::<RenderMode>().unwrap(), m);
        }
    }

    #[test]
    fn rejects_unknown() {
        assert!("nope".parse::<RenderMode>().is_err());
    }

    #[test]
    fn auto_select_is_deterministic_per_call() {
        // Two consecutive calls should agree (env doesn't change between them).
        assert_eq!(RenderMode::auto_select(), RenderMode::auto_select());
    }
}
// --- end weezterm remote features ---
