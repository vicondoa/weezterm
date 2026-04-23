# WeezTerm UX Test Findings Report

**Date:** 2026-04-22
**Platform:** Windows 11 (10.0.26200.8246), Remote Display Adapter
**Binary:** `target/debug/weezterm-gui.exe` (debug build)
**Test harness:** Python 3.11 + pywinauto + mss + pytest

---

## Summary

| Category | Tests | Passed | Failed |
|----------|-------|--------|--------|
| Startup | 4 | 4 | 0 |
| Resize | 6 | 6 | 0 |
| Maximize/Unmaximize | 6 | 6 | 0 |
| Dimension Persistence | 5 | 3 | **2** |
| SSH Mux Startup | 3 | 3 | 0 |
| SSH Mux Resize | 6 | 6 | 0 |
| SSH Mux Maximize | 4 | 4 | 0 |
| **Total** | **34** | **32** | **2** |

---

## Issues Found

### ✅ ISSUE 1: Window position always saved as (0, 0) — FIXED

**Severity:** Medium
**Test:** `test_position_preserved_on_restart`
**Status:** ✅ Fixed

**Fix applied:** Added `get_window_placement()` method to the `WindowOps` trait with
platform-specific implementations (Windows, macOS, X11). On Windows, it uses
`GetWindowRect` for position and `GetClientRect` for client dimensions in normal state,
and `GetWindowPlacement.rcNormalPosition` when maximized/fullscreen. Updated
`save_current_window_state()` to call this instead of hardcoding zeros. The window
parameter is now passed from the event handler to avoid the `self.window` being `None`
during `Destroyed` events.

---

### ✅ ISSUE 2: Normal size lost through maximize → close → reopen → restore cycle — FIXED

**Severity:** High
**Test:** `test_non_maximized_size_preserved_through_maximize_cycle`
**Status:** ✅ Fixed

**Fix applied:** The `get_window_placement()` method now uses `GetWindowPlacement.rcNormalPosition`
when the window is maximized, which returns the pre-maximize rect. This is converted from
window rect to client dimensions. When the window state is saved while maximized, the normal
dimensions are preserved. On restore, the window is created at the saved normal size, then
maximized — giving Win32 the correct `rcNormalPosition` for future unmaximize.

---

## Passing Test Highlights

### Startup Performance
- **Average startup time: ~1.9 seconds** (debug build, warm cache)
- Cold start (first run): ~9.6 seconds (expected for debug build with GPU init)
- No rendering artifacts detected at any point during startup
- No transient artifacts in the first 3 seconds after window appears

### Resize Behavior
- **All resize tests pass cleanly** — no artifacts detected after:
  - Shrinking from 1200x900 → 600x400
  - Growing from 600x400 → 1200x900
  - Rapid resize sequence (12 steps at 50ms intervals)
  - Very small (200x150) and very large (2500x1400) sizes
- Terminal background is consistently ~82% black (normal for dark theme)
- No crashes during any resize operation

### Maximize/Unmaximize
- **All maximize tests pass** — maximize/restore preserves exact dimensions
- After restore: 0px width diff, 0px height diff (perfect restoration)
- 3 maximize/restore cycles: zero drift
- WINDOWPLACEMENT normal rect is correct while maximized
- No rendering artifacts after unmaximize

---

## Issue 3 (Manual): Window Balloons on Cross-Monitor Drag

**Severity:** High
**Test:** Manual — see `tests/ux/MANUAL_TESTS.md`
**Symptom:** When dragging the window from monitor 2 to monitor 1, the window
becomes much larger than the screen. The drag outline shows a reasonable
rectangle, but the final window doesn't match it — it balloons to enormous size.

**Root cause:** `scaling_changed()` in `resize.rs:391-470` fires when the window
detects a DPI change after crossing monitor boundaries. It recalculates window
pixel dimensions to preserve terminal rows/cols at the new cell size:

```
new_pixel_width = cols × new_cell_width + padding + borders
new_pixel_height = rows × new_cell_height + padding + borders + tab_bar
```

If the new DPI is 2× the old DPI, cell sizes double, and the window pixel
dimensions roughly double. The `set_inner_size()` call at `resize.rs:378`
then resizes the window to these calculated dimensions — potentially exceeding
the target monitor's work area.

**Code path:**
1. `window/src/os/windows/window.rs:340-394` → `check_and_call_resize_if_needed()`
2. `resize.rs:67-68` → `scaling_changed()` called because DPI changed
3. `resize.rs:455-457` → `apply_scale_change()` recalculates fonts at new DPI
4. `resize.rs:192-230` → `apply_dimensions()` computes new pixel dims from preserved rows/cols
5. `resize.rs:378` → `set_inner_size()` resizes window (may exceed screen bounds)

**Suggested fix:** After `set_inner_size()` in `apply_dimensions()`, clamp the
resulting window to the target monitor's work area. If the calculated dimensions
would exceed the monitor, reduce rows/cols rather than overflow.

---

### 🟡 ISSUE 3: Missing `WM_DPICHANGED` handler — FIXED (needs manual verification)

**Severity:** High
**Test:** Manual (requires multi-monitor with different DPI — see `tests/ux/MANUAL_TESTS.md`)
**Status:** 🟡 Fixed, needs manual testing on multi-monitor setup

**Evidence:** User-reported. The drag rectangle stays reasonable but the window
"never fits in that geometry, it always balloons to be huge."

**Root cause:** `WM_DPICHANGED` is **not handled** in the window message dispatch
(`window/src/os/windows/window.rs:2971-3010`). This is the Win32 message that
Windows sends during a cross-monitor drag with a pre-calculated suggested `RECT`
in `lParam` representing the correctly-scaled window geometry for the new monitor.

The standard Win32 handler is:
```c
case WM_DPICHANGED: {
    RECT* suggested = (RECT*)lParam;
    SetWindowPos(hwnd, NULL,
        suggested->left, suggested->top,
        suggested->right - suggested->left,
        suggested->bottom - suggested->top,
        SWP_NOZORDER | SWP_NOACTIVATE);
    break;
}
```

By applying the suggested rect, the drag outline and final window position match
perfectly — Windows calculates the right size during the drag, and the outline
reflects it in real time.

Instead, WeezTerm detects the DPI change *after the fact* via
`check_and_call_resize_if_needed()` → `get_effective_dpi()`, which triggers
`scaling_changed()` → `set_inner_size()` with its own calculation that
overshoots the monitor bounds.

**Current (broken) code path:**
1. `window/src/os/windows/window.rs:340-394` → post-hoc DPI detection via `check_and_call_resize_if_needed()`
2. `resize.rs:67-68` → `scaling_changed()` called because DPI differs
3. `resize.rs:455-457` → `apply_scale_change()` recalculates fonts at new DPI (cell sizes change)
4. `resize.rs:192-230` → `apply_dimensions()` computes new pixel dims = old_rows × new_cell_size
5. `resize.rs:378` → `set_inner_size()` resizes window to calculated dimensions (may exceed screen)

**Fix:** Add a `WM_DPICHANGED` handler to `do_wnd_proc()` in
`window/src/os/windows/window.rs` that:
1. Reads the new DPI from `wParam` (LOWORD = x DPI, HIWORD = y DPI)
2. Reads the suggested `RECT` from `lParam`
3. Calls `SetWindowPos()` to apply it — makes the drag outline match final size
4. Dispatches a `Resized` event with the new DPI so `scaling_changed()` handles font recalculation

This is the standard Win32 per-monitor DPI-aware v2 behavior.

---

### 🔴 ISSUE 4: `connect --workspace` crashes SSH mux connections after ~6-8 seconds

**Severity:** High
**Test:** Discovered during SSH mux test development (not a test assertion)
**Symptom:** Running `weezterm-gui connect <domain> --workspace <non-default>` causes
the SSH mux connection to drop after ~6-8 seconds with a PDU decode EOF error.
Without `--workspace`, the connection is stable indefinitely.

**Evidence:**
- `connect jvicondo-a7` → stable 20+ seconds ✓
- `connect jvicondo-a7 --workspace ux-test` → crashes at ~8s ✗
- `connect jvicondo-bot-01 --workspace release-test` → crashes at ~11s ✗
- Reproducible with both debug and release builds
- Reproducible with any SSH host, any config

**Error output:**
```
wezterm_client::client > Error while decoding response pdu: decoding a PDU:
  decode_raw_async failed to read PDU length: EOF while reading leb128 encoded value
weezterm_gui > [...]; terminating
```

**Root cause (suspected):** When `--workspace` specifies a non-default workspace,
`spawn_tab_in_domain_if_mux_is_empty()` in `wezterm-gui/src/main.rs:289-348` needs
to create a new pane in that workspace on the remote mux server. The remote proxy
process exits (EOF on stdin/stdout) approximately 2 seconds after the workspace
creation attempt, suggesting the remote mux server closes the connection or the
spawn PDU fails in a way that kills the proxy.

The 2-second delay matches the `std::thread::sleep(Duration::new(2, 0))` in
`wezterm/src/cli/proxy.rs:82` before `std::process::exit(0)`.

**Workaround:** The UX tests avoid `--workspace` and share the default workspace.
Local process isolation is still maintained via `--config-file` and `XDG_*` env vars.

**Files to investigate:**
- `wezterm-gui/src/main.rs:289-348` — `spawn_tab_in_domain_if_mux_is_empty()`
- `wezterm/src/cli/proxy.rs` — proxy exit behavior
- `wezterm-mux-server-impl/src/sessionhandler.rs` — Spawn PDU handling for non-default workspaces

### SSH Mux Test Results (all passing)
- **All 13 SSH mux tests pass** with the corrected test setup (no `--workspace`)
- SSH mux startup: ~1.6-8.4s (variable due to SSH negotiation)
- No rendering artifacts after startup, resize, or unmaximize over SSH mux
- Resize behavior identical to local: no artifacts, no crashes
- Maximize/restore cycles: zero drift, perfect dimension preservation
- Rapid resize over SSH mux: stable, no crash

### Dimension Persistence (Partial)
- Window width and height ARE preserved across restarts ✓
- Maximized state IS preserved across restarts ✓
- `window-state.json` IS written on graceful close ✓
- Position (x, y) is NOT preserved (see Issue 1) ✗
- Normal dimensions are NOT preserved through maximize cycle (see Issue 2) ✗

### Resize Visual Quality
- Terminal content is stretched/distorted during resize before settling to final size
- Multiple intermediate redraws visible during a single resize operation
- See Issue 5 below

---

## ✅ Issue 5: Content Stretching / Multiple Redraws During Resize — FIXED

**Severity:** Medium (visual quality)
**Status:** ✅ Fixed

**Fix applied:** During live resize (user dragging window edge), the terminal content
recalculation is now deferred. The WebGPU surface is reconfigured to match the new
window dimensions (required by the GPU driver), but instead of rendering terminal
content on every intermediate step, a cleared (black) frame is presented immediately.
When the user releases the mouse button (`WM_EXITSIZEMOVE`), the full terminal
resize + repaint happens in one clean step. This eliminates content stretching,
multiple intermediate redraws, and the jarring visual experience. The change uses
the existing `live_resizing` flag (already tracked on all platforms via
`WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE` on Windows, `windowWillStartLiveResize` on macOS,
`ConfigureNotify` on X11).

---

## Improvement Recommendations (Updated)

### ✅ Priority 1: Fix the oversized-window-on-restore bug (Issue 2) — DONE
### ✅ Priority 2: Fix window position persistence (Issue 1) — DONE
### ✅ Priority 3: Add WM_DPICHANGED handler (Issue 3) — DONE (needs manual verification)
### ✅ Priority 4: Fix content stretching during resize (Issue 5) — DONE

### Remaining: Fix SSH mux connection stability (Issue 4)
SSH mux connections via `connect` with an isolated config drop after ~6 seconds.
This blocks all SSH mux resize/maximize testing and likely affects users.
**Files to investigate:**
- `wezterm-client/src/client.rs` — PDU decode error handling
- `wezterm-client/src/domain.rs` — ClientDomain connection lifecycle
- `codec/src/lib.rs` — codec version compatibility

### Remaining: CI integration
Add these UX tests to the `weezterm_build.yml` workflow (Windows job) so
regressions are caught automatically.

---

## Test Harness Location

```
tests/ux/
├── conftest.py           # fixtures with process isolation
├── helpers/
│   ├── app.py            # WeezTermApp: isolated process lifecycle
│   ├── window_ops.py     # Win32 API wrappers
│   ├── screenshot.py     # mss capture + artifact detection
│   └── timing.py         # measurement utilities
├── test_startup.py       # 4 tests
├── test_resize.py        # 6 tests
├── test_maximize.py      # 6 tests
├── test_dimensions.py    # 5 tests
├── test_ssh_mux.py       # 13 tests (SSH mux connection + resize + maximize)
├── MANUAL_TESTS.md       # multi-monitor manual test checklist
├── requirements.txt
└── test-results/         # screenshots from latest run
```

### Running the tests
```bash
cd tests/ux
pip install -r requirements.txt
python -m pytest -v -s
```
