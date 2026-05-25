use super::*;
use glium::backend::Backend;
use std::ffi::CStr;
use std::io::Error as IoError;
use std::os::raw::c_void;
use std::ptr::{null, null_mut};
use winapi::shared::windef::*;
use winapi::um::libloaderapi::{GetModuleHandleW, *};
use winapi::um::wingdi::*;
use winapi::um::winuser::*;

pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/wgl_bindings.rs"));
}
pub mod ffiextra {
    include!(concat!(env!("OUT_DIR"), "/wgl_extra_bindings.rs"));
}

struct WglWrapper {
    lib: libloading::Library,
    wgl: ffi::Wgl,
    ext: Option<ffiextra::Wgl>,
}

type GetProcAddressFunc =
    unsafe extern "system" fn(*const std::os::raw::c_char) -> *const std::os::raw::c_void;

impl Drop for WglWrapper {
    fn drop(&mut self) {
        log::trace!("dropping WglWrapper and libloading {:?}", self.lib);
    }
}

impl WglWrapper {
    fn load() -> anyhow::Result<Self> {
        let class_name = wide_string("wezterm wgl extension probing window");
        let h_inst = unsafe { GetModuleHandleW(null()) };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_inst,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };

        if unsafe { RegisterClassW(&class) } == 0 {
            let err = IoError::last_os_error();
            match err.raw_os_error() {
                Some(code)
                    if code == winapi::shared::winerror::ERROR_CLASS_ALREADY_EXISTS as i32 => {}
                _ => return Err(err.into()),
            }
        }

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                class_name.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1024,
                768,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if hwnd.is_null() {
            let err = IoError::last_os_error();
            anyhow::bail!("CreateWindowExW: {}", err);
        }

        let mut state = GlState::create_basic(WglWrapper::create()?, hwnd)?;

        unsafe {
            state.make_current();
        }

        let _ = state.wgl.as_mut().unwrap().load_ext();

        state.make_not_current();

        Ok(state.into_wrapper())
    }

    fn create() -> anyhow::Result<Self> {
        // --- weezterm remote features ---
        // `opengl32.dll` is a Windows "Known DLL", so the loader
        // would normally always pull it from System32 regardless of
        // any LoadLibrary path tricks. We override that via a
        // `<file name="opengl32.dll"/>` entry in the application
        // SxS manifest (assets/windows/manifest.manifest), which
        // tells the loader that opengl32 is a private assembly file
        // and to load it from the application directory instead.
        // The build script (wezterm-gui/build.rs) places our bundled
        // Mesa 26.x build there (opengl32.dll + libgallium_wgl.dll),
        // so this `LoadLibraryW` ends up pulling Mesa's loader, which
        // in turn loads `libgallium_wgl.dll` for the actual driver.
        //
        // When we're routing GL via Mesa for the RDP/WARP path we
        // also want llvmpipe (pure-CPU rasterizer) rather than the
        // default GLonD3D12 — under RDP, GLonD3D12 would just send
        // commands to D3D12 → WARP and we'd be back where we started.
        // Set GALLIUM_DRIVER before the first WGL call so Mesa picks
        // up the override at gallium initialization time.
        if crate::configuration::prefer_swrast() && std::env::var_os("GALLIUM_DRIVER").is_none() {
            // SAFETY: set_var is only `unsafe` from Rust 2024+ in a
            // multithreaded process. This is called from the GUI
            // thread very early in window setup (before any other
            // thread reads GALLIUM_DRIVER).
            std::env::set_var("GALLIUM_DRIVER", "llvmpipe");
            log::debug!("set GALLIUM_DRIVER=llvmpipe for Mesa SW rendering on RDP");
        }
        // --- end weezterm remote features ---
        let lib = unsafe { libloading::Library::new("opengl32.dll") }.map_err(|e| {
            log::error!("{:?}", e);
            e
        })?;
        log::trace!("loaded {:?}", lib);

        let get_proc_address: libloading::Symbol<GetProcAddressFunc> =
            unsafe { lib.get(b"wglGetProcAddress\0")? };
        let wgl = ffi::Wgl::load_with(|s: &'static str| {
            let sym_name = std::ffi::CString::new(s).expect("symbol to be cstring compatible");
            if let Ok(sym) = unsafe { lib.get(sym_name.as_bytes_with_nul()) } {
                return *sym;
            }
            unsafe { get_proc_address(sym_name.as_ptr()) }
        });
        Ok(Self {
            lib,
            wgl,
            ext: None,
        })
    }

    fn load_ext(&mut self) -> anyhow::Result<()> {
        let get_proc_address: libloading::Symbol<GetProcAddressFunc> =
            unsafe { self.lib.get(b"wglGetProcAddress\0")? };

        self.ext
            .replace(ffiextra::Wgl::load_with(|s: &'static str| {
                let sym_name = std::ffi::CString::new(s).expect("symbol to be cstring compatible");
                if let Ok(sym) = unsafe { self.lib.get(sym_name.as_bytes_with_nul()) } {
                    return *sym;
                }
                unsafe { get_proc_address(sym_name.as_ptr()) }
            }));

        Ok(())
    }
}

pub struct GlState {
    wgl: Option<WglWrapper>,
    hdc: HDC,
    rc: ffi::types::HGLRC,
}

fn has_extension(extensions: &str, wanted: &str) -> bool {
    extensions.split(' ').find(|&ext| ext == wanted).is_some()
}

impl GlState {
    fn into_wrapper(mut self) -> WglWrapper {
        self.delete();
        self.wgl.take().unwrap()
    }

    pub fn create(window: HWND) -> anyhow::Result<Self> {
        let wgl = WglWrapper::load()?;

        if let Some(ext) = wgl.ext.as_ref() {
            let hdc = unsafe { GetDC(window) };

            fn cstr(data: *const i8) -> String {
                let data = unsafe { CStr::from_ptr(data).to_bytes().to_vec() };
                String::from_utf8(data).unwrap()
            }

            let extensions = if ext.GetExtensionsStringARB.is_loaded() {
                unsafe { cstr(ext.GetExtensionsStringARB(hdc as *const _)) }
            } else if ext.GetExtensionsStringEXT.is_loaded() {
                unsafe { cstr(ext.GetExtensionsStringEXT()) }
            } else {
                "".to_owned()
            };
            log::trace!("opengl extensions: {:?}", extensions);

            if has_extension(&extensions, "WGL_ARB_pixel_format") {
                // --- weezterm remote features ---
                // First try with full attributes (incl. 4x MSAA).
                // Mesa llvmpipe's WGL backend on RDP does not advertise
                // multisample formats, so retry without MSAA before
                // falling back to the legacy `create_basic` path —
                // `create_basic` uses ChoosePixelFormat (1.x) which
                // can land on a transparent-composed pixel format on
                // Mesa, leaving the window invisible.
                match Self::create_ext_with_msaa(wgl, extensions.clone(), hdc, true) {
                    Ok(state) => return Ok(state),
                    Err(err_msaa) => {
                        log::warn!(
                            "create_ext (with MSAA) failed ({}); retrying without MSAA",
                            err_msaa
                        );
                    }
                }
                let wgl_retry = WglWrapper::load()?;
                return match Self::create_ext_with_msaa(wgl_retry, extensions, hdc, false) {
                    Ok(state) => Ok(state),
                    Err(err) => {
                        log::warn!(
                            "failed to created extended OpenGL context \
                            ({}), fall back to basic",
                            err
                        );
                        let wgl = WglWrapper::load()?;
                        Self::create_basic(wgl, window)
                    }
                };
                // --- end weezterm remote features ---
            }
        }

        Self::create_basic(wgl, window)
    }

    // --- weezterm remote features ---
    fn create_ext_with_msaa(
        wgl: WglWrapper,
        extensions: String,
        hdc: HDC,
        msaa: bool,
    ) -> anyhow::Result<Self> {
        // --- end weezterm remote features ---
        use ffiextra::*;

        let mut attribs: Vec<i32> = vec![
            DRAW_TO_WINDOW_ARB as i32,
            1,
            SUPPORT_OPENGL_ARB as i32,
            1,
            DOUBLE_BUFFER_ARB as i32,
            1,
            PIXEL_TYPE_ARB as i32,
            TYPE_RGBA_ARB as i32,
            COLOR_BITS_ARB as i32,
            24,
            ALPHA_BITS_ARB as i32,
            8,
            DEPTH_BITS_ARB as i32,
            24,
            STENCIL_BITS_ARB as i32,
            8,
        ];
        // --- weezterm remote features ---
        if msaa {
            attribs.push(SAMPLE_BUFFERS_ARB as i32);
            attribs.push(1);
            attribs.push(SAMPLES_ARB as i32);
            attribs.push(4);
        }
        // --- end weezterm remote features ---

        if has_extension(&extensions, "WGL_ARB_framebuffer_sRGB") {
            log::trace!("will request FRAMEBUFFER_SRGB_CAPABLE_ARB");
            attribs.push(FRAMEBUFFER_SRGB_CAPABLE_ARB as i32);
            attribs.push(1);
        } else if has_extension(&extensions, "WGL_EXT_framebuffer_sRGB") {
            log::trace!("will request FRAMEBUFFER_SRGB_CAPABLE_EXT");
            attribs.push(FRAMEBUFFER_SRGB_CAPABLE_EXT as i32);
            attribs.push(1);
        }

        attribs.push(0);

        let mut format_id = 0;
        let mut num_formats = 0;

        let res = unsafe {
            wgl.ext.as_ref().unwrap().ChoosePixelFormatARB(
                hdc as _,
                attribs.as_ptr(),
                null(),
                1,
                &mut format_id,
                &mut num_formats,
            )
        };
        if res == 0 {
            anyhow::bail!("ChoosePixelFormatARB returned 0");
        }

        if num_formats == 0 {
            anyhow::bail!("ChoosePixelFormatARB returned 0 formats");
        }
        // --- weezterm remote features ---
        log::debug!(
            "ChoosePixelFormatARB returned format_id={} (msaa={})",
            format_id,
            msaa
        );
        // --- end weezterm remote features ---

        let mut pfd: PIXELFORMATDESCRIPTOR = unsafe { std::mem::zeroed() };

        let res = unsafe {
            DescribePixelFormat(
                hdc,
                format_id,
                std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as _,
                &mut pfd,
            )
        };
        if res == 0 {
            // --- weezterm remote features ---
            // Mesa llvmpipe's WGL ICD invents pixel format IDs that
            // don't exist in the system GDI format database, so the
            // Win32 DescribePixelFormat call returns ERROR_INVALID_PARAMETER
            // (87). SetPixelFormat itself only uses the format ID
            // (the PFD argument is informational), so we can skip
            // the describe step and pass a zeroed PFD. Fill in
            // nSize/nVersion to keep SetPixelFormat happy.
            log::debug!(
                "DescribePixelFormat({}) failed ({}); proceeding with zeroed PFD \
                 (Mesa-WGL ICD path)",
                format_id,
                std::io::Error::last_os_error()
            );
            pfd = unsafe { std::mem::zeroed() };
            pfd.nSize = std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16;
            pfd.nVersion = 1;
            // --- end weezterm remote features ---
        }

        let res = unsafe { SetPixelFormat(hdc, format_id, &pfd) };
        if res == 0 {
            anyhow::bail!(
                "SetPixelFormat function failed: {}",
                std::io::Error::last_os_error()
            );
        }

        // --- weezterm remote features ---
        // Try the requested OpenGL version first (4.5 core), then fall
        // back to lower versions. Mesa llvmpipe builds for Windows
        // (mesa.fdossena.com) only support up to OpenGL 3.3, so we
        // need this fallback in any setup where Mesa is the
        // OpenGL driver. glium itself only requires 3.0.
        let candidates: &[(i32, i32)] = &[(4, 5), (3, 3), (3, 2), (3, 0)];
        let mut last_err: Option<u32> = None;
        let mut rc = std::ptr::null();
        for &(maj, min) in candidates {
            let mut attribs = vec![
                CONTEXT_MAJOR_VERSION_ARB as i32,
                maj,
                CONTEXT_MINOR_VERSION_ARB as i32,
                min,
                CONTEXT_PROFILE_MASK_ARB as i32,
                CONTEXT_CORE_PROFILE_BIT_ARB as i32,
            ];

            if has_extension(&extensions, "WGL_ARB_create_context_robustness") {
                log::trace!("requesting robustness features");
                attribs.push(CONTEXT_RESET_NOTIFICATION_STRATEGY_ARB as i32);
                attribs.push(LOSE_CONTEXT_ON_RESET_ARB as i32);
                attribs.push(CONTEXT_FLAGS_ARB as i32);
                attribs.push(CONTEXT_ROBUST_ACCESS_BIT_ARB as i32);
            }
            attribs.push(0);

            rc = unsafe {
                wgl.ext.as_ref().unwrap().CreateContextAttribsARB(
                    hdc as _,
                    null(),
                    attribs.as_ptr(),
                )
            };

            if !rc.is_null() {
                log::debug!(
                    "CreateContextAttribsARB succeeded for OpenGL {}.{} core",
                    maj,
                    min
                );
                break;
            }
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            last_err = Some(err);
            log::debug!(
                "CreateContextAttribsARB({}.{} core) failed, GetLastError={} {:x}",
                maj,
                min,
                err,
                err
            );
        }

        if rc.is_null() {
            anyhow::bail!(
                "CreateContextAttribsARB failed for all candidate \
                 OpenGL versions (last GetLastError={:?})",
                last_err
            );
        }
        // --- end weezterm remote features ---

        unsafe {
            wgl.wgl.MakeCurrent(hdc as *mut _, rc);
        }

        Ok(Self {
            wgl: Some(wgl),
            rc,
            hdc,
        })
    }

    fn create_basic(wgl: WglWrapper, window: HWND) -> anyhow::Result<Self> {
        let hdc = unsafe { GetDC(window) };

        let pfd = PIXELFORMATDESCRIPTOR {
            nSize: std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16,
            nVersion: 1,
            dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
            iPixelType: PFD_TYPE_RGBA,
            cColorBits: 24,
            cRedBits: 0,
            cRedShift: 0,
            cGreenBits: 0,
            cGreenShift: 0,
            cBlueBits: 0,
            cBlueShift: 0,
            cAlphaBits: 8,
            cAlphaShift: 0,
            cAccumBits: 0,
            cAccumRedBits: 0,
            cAccumGreenBits: 0,
            cAccumBlueBits: 0,
            cAccumAlphaBits: 0,
            cDepthBits: 24,
            cStencilBits: 8,
            cAuxBuffers: 0,
            iLayerType: PFD_MAIN_PLANE,
            bReserved: 0,
            dwLayerMask: 0,
            dwVisibleMask: 0,
            dwDamageMask: 0,
        };
        let format = unsafe { ChoosePixelFormat(hdc, &pfd) };
        unsafe {
            SetPixelFormat(hdc, format, &pfd);
        }

        let rc = unsafe { wgl.wgl.CreateContext(hdc as *mut _) };
        unsafe {
            wgl.wgl.MakeCurrent(hdc as *mut _, rc);
        }

        Ok(Self {
            wgl: Some(wgl),
            rc,
            hdc,
        })
    }

    fn make_not_current(&self) {
        if let Some(wgl) = self.wgl.as_ref() {
            unsafe {
                wgl.wgl.MakeCurrent(self.hdc as *mut _, std::ptr::null());
            }
        }
    }

    fn delete(&mut self) {
        self.make_not_current();
        if let Some(wgl) = self.wgl.as_ref() {
            unsafe {
                wgl.wgl.DeleteContext(self.rc);
            }
        }
    }
}

impl Drop for GlState {
    fn drop(&mut self) {
        self.delete();
    }
}

unsafe impl glium::backend::Backend for GlState {
    fn resize(&self, _: (u32, u32)) {
        todo!()
    }

    fn swap_buffers(&self) -> Result<(), glium::SwapBuffersError> {
        unsafe {
            SwapBuffers(self.hdc);
        }
        Ok(())
    }

    unsafe fn get_proc_address(&self, symbol: &str) -> *const c_void {
        let sym_name = std::ffi::CString::new(symbol).expect("symbol to be cstring compatible");
        if let Ok(sym) = self
            .wgl
            .as_ref()
            .unwrap()
            .lib
            .get(sym_name.as_bytes_with_nul())
        {
            //eprintln!("{} -> {:?}", symbol, sym);
            return *sym;
        }
        let res = self
            .wgl
            .as_ref()
            .unwrap()
            .wgl
            .GetProcAddress(sym_name.as_ptr()) as *const c_void;
        // eprintln!("{} -> {:?}", symbol, res);
        res
    }

    fn get_framebuffer_dimensions(&self) -> (u32, u32) {
        unimplemented!();
    }

    fn is_current(&self) -> bool {
        unsafe { self.wgl.as_ref().unwrap().wgl.GetCurrentContext() == self.rc }
    }

    unsafe fn make_current(&self) {
        self.wgl
            .as_ref()
            .unwrap()
            .wgl
            .MakeCurrent(self.hdc as *mut _, self.rc);
    }
}
