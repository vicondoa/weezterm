// --- weezterm remote features ---
//! Dedicated thread for periodic cursor-blink invalidations.
//!
//! Decouples the timer-driven repaint cadence from the WindowProc message
//! loop so the cursor blink remains visible during sustained scroll / paint
//! load that would otherwise starve the existing smol-`Timer` driven blink
//! schedule (rendered via [`crate::termwindow::TermWindow::scheduled_animation`]).
//!
//! This is the design doc's **Option 6A** ("light decoupled paint"). The
//! existing animation/blink scheduling in
//! `wezterm-gui/src/termwindow/render/paint.rs` is preserved as-is; this
//! thread runs alongside it and posts an `InvalidateRect` on a fixed
//! cadence. On Windows, `InvalidateRect` is documented as safe to call from
//! any thread, and duplicate invalidations coalesce in the message queue,
//! so the redundant signal is benign.
//!
//! The full "render thread" refactor (Option 6B) — which would move the
//! `do_paint_*` body off the UI thread entirely — remains a future option.
//! See `docs/windows-rendering-design.md` §6 Phase 6.
//!
//! Reference: Ghostty `src/renderer/Thread.zig:19-64`.

use std::time::Duration;

#[cfg(windows)]
mod imp {
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread::{Builder, JoinHandle};
    use std::time::Duration;

    use winapi::shared::windef::HWND;
    use winapi::um::winuser::InvalidateRect;

    /// Newtype that asserts the `HWND` may be moved across threads.
    ///
    /// HWND is a non-thread-affine handle in Windows; `InvalidateRect` is
    /// explicitly thread-safe and simply posts a `WM_PAINT` to the window's
    /// owning thread. If the window has been destroyed by the time the
    /// thread wakes, the call is a harmless no-op (it returns FALSE).
    #[derive(Copy, Clone)]
    struct SendHwnd(HWND);

    // SAFETY: HWND values are not bound to a thread; they may be passed
    // across threads as opaque tokens. The only operation this module
    // performs on the handle is `InvalidateRect`, which Microsoft
    // documents as safe to call from any thread.
    unsafe impl Send for SendHwnd {}
    unsafe impl Sync for SendHwnd {}

    pub struct CursorBlinkThread {
        interval_ms: Arc<AtomicU64>,
        enabled: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl CursorBlinkThread {
        /// Spawn a new cursor-blink thread for the given window handle.
        ///
        /// Returns a thread handle that will keep the worker alive until
        /// dropped. The worker wakes every `initial_interval` and posts an
        /// `InvalidateRect` to `hwnd`. Use [`Self::set_interval`] to update
        /// the cadence at runtime (e.g. when config reloads). If
        /// `initial_interval` is zero (blink disabled in config), the
        /// thread is not spawned.
        pub fn spawn(hwnd: HWND, initial_interval: Duration) -> Option<Self> {
            let initial_ms = initial_interval.as_millis() as u64;
            if initial_ms == 0 {
                log::debug!(
                    "[render] cursor blink disabled (interval=0); not spawning blink thread"
                );
                return None;
            }
            let interval_ms = Arc::new(AtomicU64::new(clamp_interval(initial_ms)));
            let enabled = Arc::new(AtomicBool::new(true));
            let send_hwnd = SendHwnd(hwnd);

            let interval_for_thread = Arc::clone(&interval_ms);
            let enabled_for_thread = Arc::clone(&enabled);

            let spawn_result =
                Builder::new()
                    .name("weezterm-cursor-blink".into())
                    .spawn(move || {
                        log::debug!("[render] cursor blink thread started");
                        while enabled_for_thread.load(Ordering::Relaxed) {
                            let ms = interval_for_thread.load(Ordering::Relaxed);
                            // Sleep in small chunks so a config reload that
                            // shortens the interval, or a shutdown, is observed
                            // promptly without having to wait for the previous
                            // long sleep to elapse.
                            let mut remaining = ms;
                            while remaining > 0 && enabled_for_thread.load(Ordering::Relaxed) {
                                let slice = remaining.min(50);
                                std::thread::sleep(Duration::from_millis(slice));
                                remaining = remaining.saturating_sub(slice);
                            }
                            if !enabled_for_thread.load(Ordering::Relaxed) {
                                break;
                            }
                            // SAFETY: see SendHwnd doc-comment.
                            unsafe {
                                InvalidateRect(send_hwnd.0, null(), 0);
                            }
                        }
                        log::debug!("[render] cursor blink thread exiting");
                    });

            match spawn_result {
                Ok(handle) => Some(Self {
                    interval_ms,
                    enabled,
                    handle: Some(handle),
                }),
                Err(err) => {
                    log::warn!(
                        "[render] failed to spawn cursor blink thread: {err}; falling back to \
                         the existing smol-timer blink schedule only"
                    );
                    None
                }
            }
        }

        /// Update the wakeup cadence. Useful when a config reload changes
        /// `cursor_blink_rate`.
        pub fn set_interval(&self, interval: Duration) {
            self.interval_ms.store(
                clamp_interval(interval.as_millis() as u64),
                Ordering::Relaxed,
            );
        }
    }

    impl Drop for CursorBlinkThread {
        fn drop(&mut self) {
            self.enabled.store(false, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// Bound the blink interval to a sane range.
    ///
    /// - 50 ms is a hard floor to avoid a runaway loop posting 1000s of
    ///   invalidations per second if the config is misconfigured.
    /// - 60 s is an upper bound so a Drop / config reload is always
    ///   observed within at most one minute.
    fn clamp_interval(ms: u64) -> u64 {
        ms.clamp(50, 60_000)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::time::Duration;

    /// Stub for non-Windows targets. The cursor-blink starvation problem is
    /// specific to the Win32 WindowProc message loop; macOS and X11/Wayland
    /// drive the redraw clock differently and don't exhibit the same
    /// starvation pattern. Keeping a no-op type here lets the call sites in
    /// `termwindow/mod.rs` stay platform-agnostic.
    pub struct CursorBlinkThread;

    impl CursorBlinkThread {
        pub fn set_interval(&self, _interval: Duration) {}
    }
}

#[cfg(windows)]
pub use imp::CursorBlinkThread;

#[cfg(not(windows))]
pub use imp::CursorBlinkThread;

/// Spawn a cursor-blink thread for the given window. The returned handle
/// keeps the worker alive until dropped. Returns `None` on platforms where
/// the thread is not used or if spawning fails.
#[cfg(windows)]
pub fn spawn_for_window(
    window: &::window::Window,
    interval: Duration,
) -> Option<CursorBlinkThread> {
    use ::window::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = match window.window_handle() {
        Ok(h) => h,
        Err(err) => {
            log::warn!("[render] cursor blink thread: window_handle() failed: {err}; not spawning");
            return None;
        }
    };
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => {
            let hwnd = win32.hwnd.get() as winapi::shared::windef::HWND;
            CursorBlinkThread::spawn(hwnd, interval)
        }
        other => {
            log::debug!("[render] cursor blink thread: non-Win32 handle ({other:?}); not spawning");
            None
        }
    }
}

/// Non-Windows stub: never spawns.
#[cfg(not(windows))]
pub fn spawn_for_window(
    _window: &::window::Window,
    _interval: Duration,
) -> Option<CursorBlinkThread> {
    None
}
// --- end weezterm remote features ---
