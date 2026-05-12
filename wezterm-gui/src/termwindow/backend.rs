//! Rendering-backend abstraction for `TermWindow`.
//!
//! Phase 4 of `docs/windows-rendering-design.md` introduces a third
//! rendering mode (`SoftwareRdp`) alongside the existing OpenGL/glium and
//! wgpu paths. This module replaces the previous
//! `webgpu: Option<Rc<WebGpuState>>` field with a `RenderBackend` enum so
//! that all backends have a single attachment point on `TermWindow`.
//!
//! Note: the OpenGL/glium backend is still attached via `TermWindow.gl`;
//! we did not roll it into this enum because doing so would touch many
//! upstream call sites that are not relevant to phase 4. When the
//! OpenGL path is finally removed (phase 7) the `gl` field can be folded
//! in here.

use crate::termwindow::webgpu::WebGpuState;
use std::rc::Rc;

/// Active GPU/CPU rendering backend held by a `TermWindow`.
///
/// The previous code held `Option<Rc<WebGpuState>>` directly on
/// `TermWindow`. This enum is the single dispatch point that
/// `paint_window` and friends switch on.
pub enum RenderBackend {
    /// No backend is attached. Either we are running on the OpenGL/glium
    /// path (the renderer is in `TermWindow.gl`), or we are mid-init.
    None,
    /// wgpu (Mode A `WgpuDComp` / Mode B `WgpuClassic`).
    WebGpu(Rc<WebGpuState>),
}

impl RenderBackend {
    /// Returns the wgpu state, if `self` is `WebGpu(_)`.
    pub fn webgpu(&self) -> Option<&Rc<WebGpuState>> {
        match self {
            Self::WebGpu(w) => Some(w),
            Self::None => None,
        }
    }

    /// True if any non-glium backend is active.
    #[allow(dead_code)] // Will be consumed by Phase 4b dispatch + Phase 4d tests.
    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}
