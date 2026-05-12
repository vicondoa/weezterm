pub mod connection;
pub mod event;
mod extra_constants;
mod keycodes;
mod wgl;
pub mod window;

pub use self::window::*;
pub use connection::*;
pub use event::*;

/// Convert a rust string to a windows wide string
pub fn wide_string(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Returns true if we are running in an RDP session.
/// See <https://docs.microsoft.com/en-us/windows/win32/termserv/detecting-the-terminal-services-environment>
pub fn is_running_in_rdp_session() -> bool {
    use winapi::shared::minwindef::DWORD;
    use winapi::um::processthreadsapi::{GetCurrentProcessId, ProcessIdToSessionId};
    use winapi::um::winuser::{GetSystemMetrics, SM_REMOTESESSION};
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    if unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0 {
        return true;
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let terminal_server =
        match hklm.open_subkey("SYSTEM\\CurrentControlSet\\Control\\Terminal Server\\") {
            Ok(k) => k,
            Err(_) => return false,
        };

    let glass_session_id: DWORD = match terminal_server.get_value("GlassSessionId") {
        Ok(sess) => sess,
        Err(_) => return false,
    };

    unsafe {
        let mut current_session = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut current_session) != 0 {
            // If we're not the glass session then we're a remote session
            current_session != glass_session_id
        } else {
            false
        }
    }
}

// --- weezterm remote features ---

/// A description of a single DXGI adapter, suitable for logging.
#[derive(Debug, Clone)]
pub struct DxgiAdapterInfo {
    pub description: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// Matches `DXGI_ADAPTER_FLAG_SOFTWARE` — true for WARP and other
    /// purely-software adapters.
    pub is_software: bool,
}

/// Returns the Windows build number (e.g. 19041 for the May 2020 update,
/// 22000+ for Windows 11). Returns 0 if the call fails.
///
/// Reads `CurrentBuildNumber` from
/// `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`. The value is a
/// `REG_SZ` string like "26200"; we parse it as `u32`.
pub fn windows_build_number() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = match hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion") {
        Ok(k) => k,
        Err(_) => return 0,
    };
    let value: String = match key.get_value("CurrentBuildNumber") {
        Ok(v) => v,
        Err(_) => return 0,
    };
    value.parse::<u32>().unwrap_or(0)
}

/// Enumerates DXGI adapters via `IDXGIFactory1::EnumAdapters1`.
/// Returns an empty `Vec` on failure (e.g. DXGI not present, factory
/// creation failed, no adapters available).
pub fn enumerate_dxgi_adapters() -> Vec<DxgiAdapterInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;
    use winapi::shared::dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_DESC1,
        DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    use winapi::shared::winerror::{DXGI_ERROR_NOT_FOUND, SUCCEEDED};
    use winapi::Interface;

    let mut adapters = Vec::new();
    unsafe {
        let mut factory: *mut IDXGIFactory1 = ptr::null_mut();
        let hr = CreateDXGIFactory1(
            &IDXGIFactory1::uuidof(),
            &mut factory as *mut *mut IDXGIFactory1 as *mut *mut _,
        );
        if !SUCCEEDED(hr) || factory.is_null() {
            return adapters;
        }

        let mut index: u32 = 0;
        loop {
            let mut adapter: *mut IDXGIAdapter1 = ptr::null_mut();
            let hr = (*factory).EnumAdapters1(index, &mut adapter);
            if hr == DXGI_ERROR_NOT_FOUND {
                break;
            }
            if !SUCCEEDED(hr) || adapter.is_null() {
                break;
            }

            let mut desc: DXGI_ADAPTER_DESC1 = std::mem::zeroed();
            let dhr = (*adapter).GetDesc1(&mut desc);
            if SUCCEEDED(dhr) {
                let len = desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let description = OsString::from_wide(&desc.Description[..len])
                    .to_string_lossy()
                    .into_owned();
                adapters.push(DxgiAdapterInfo {
                    description,
                    vendor_id: desc.VendorId,
                    device_id: desc.DeviceId,
                    is_software: (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0,
                });
            }

            (*adapter).Release();
            index += 1;
        }

        (*factory).Release();
    }

    adapters
}

/// Returns true if every available DXGI adapter is one of the well-known
/// virtual GPU descriptions: "Microsoft Basic Display",
/// "Microsoft Hyper-V Video", or "Microsoft Remote Display Adapter".
///
/// Description match is case-insensitive and uses substring containment;
/// the names are stable since Windows 10. Returns false if no adapters
/// are enumerated (cannot prove the negative).
pub fn only_virtual_gpus_available() -> bool {
    let adapters = enumerate_dxgi_adapters();
    if adapters.is_empty() {
        return false;
    }

    const VIRTUAL_NAMES: &[&str] = &[
        "microsoft basic display",
        "microsoft hyper-v video",
        "microsoft remote display adapter",
    ];

    adapters.iter().all(|a| {
        let desc = a.description.to_lowercase();
        VIRTUAL_NAMES.iter().any(|vn| desc.contains(vn))
    })
}

// --- end weezterm remote features ---
