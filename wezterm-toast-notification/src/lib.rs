mod dbus;
mod macos;
mod windows;

#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub title: String,
    pub message: String,
    pub url: Option<String>,
    pub timeout: Option<std::time::Duration>,
    // --- weezterm remote features ---
    /// If set, the toast should be dismissed when this flag becomes true.
    pub cancel_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    // --- end weezterm remote features ---
}

impl ToastNotification {
    pub fn show(self) {
        show(self)
    }
}

#[cfg(windows)]
use crate::windows as backend;
#[cfg(all(not(target_os = "macos"), not(windows)))]
use dbus as backend;
#[cfg(target_os = "macos")]
use macos as backend;

mod nop {
    use super::*;

    #[allow(dead_code)]
    pub fn show_notif(_: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

pub fn show(notif: ToastNotification) {
    if let Err(err) = backend::show_notif(notif) {
        log::error!("Failed to show notification: {}", err);
    }
}

pub fn persistent_toast_notification_with_click_to_open_url(title: &str, message: &str, url: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: Some(url.to_string()),
        timeout: None,
        cancel_flag: None,
    });
}

pub fn persistent_toast_notification(title: &str, message: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: None,
        timeout: None,
        cancel_flag: None,
    });
}

// --- weezterm remote features ---
use std::sync::{Arc, Mutex};

/// Global cancel flag for the current confirm-open-url toast.
/// When a new URL confirmation is requested, the previous one is cancelled.
static CONFIRM_CANCEL: std::sync::LazyLock<Arc<Mutex<Arc<std::sync::atomic::AtomicBool>>>> =
    std::sync::LazyLock::new(|| {
        Arc::new(Mutex::new(Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        ))))
    });

/// Show a confirmation toast for opening a URL.
///
/// - Cancels any previous pending confirmation toast
/// - Shows a new toast with a "Show" button that opens the URL on click
/// - Toast stays visible for `timeout_secs` seconds
/// - On Windows, uses `scenario="urgentMessage"` so the toast stays in foreground
pub fn show_confirm_open_url(url: &str, timeout_secs: u64) {
    // Cancel the previous confirm toast (if any)
    {
        let mut guard = CONFIRM_CANCEL.lock().unwrap();
        guard.store(true, std::sync::atomic::Ordering::SeqCst);
        // Replace with a fresh flag for the new toast
        *guard = Arc::new(std::sync::atomic::AtomicBool::new(false));
    }
    let cancel_flag = CONFIRM_CANCEL.lock().unwrap().clone();

    show(ToastNotification {
        title: "Open URL?".to_string(),
        message: format!("Remote host wants to open:\n{}", url),
        url: Some(url.to_string()),
        timeout: Some(std::time::Duration::from_secs(timeout_secs)),
        cancel_flag: Some(cancel_flag),
    });
}
// --- end weezterm remote features ---

#[cfg(target_os = "macos")]
pub use macos::initialize as macos_initialize;
