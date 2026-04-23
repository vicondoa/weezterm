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

### 🔴 ISSUE 1: Window position always saved as (0, 0)

**Severity:** Medium
**Test:** `test_position_preserved_on_restart`
**Symptom:** Window reopens at default OS position instead of where the user placed it.

**Evidence:**
- Window placed at (250, 175)
- Saved `window-state.json` shows `"x": 0, "y": 0`
- After restart, window opens at (286, 286) — the OS default cascade position

**Root cause:** `save_current_window_state()` in `wezterm-gui/src/termwindow/mod.rs:2015-2023`
hardcodes `x: 0, y: 0` with a comment "Will be overridden below if we can get position" — but
**no code below actually overrides x/y**. The position is never populated.

```rust
// BUG: x and y are always 0
let state = crate::window_state_persistence::SavedWindowState {
    x: 0, // Will be overridden below if we can get position  <-- NEVER OVERRIDDEN
    y: 0,
    width: self.dimensions.pixel_width,
    height: self.dimensions.pixel_height,
    ...
};
```

**Fix:** The `TermWindow` needs access to the window's screen position. Options:
1. Add a `window_position: (isize, isize)` field to `TermWindow` that gets updated
   on move events, then use it in `save_current_window_state()`
2. Query the window position via the `Window` handle at save time (requires adding
   a `get_position()` method to the `WindowOps` trait)
3. Use `WINDOWPLACEMENT` which contains the normal position — this is the best
   approach because it also solves Issue 2.

---

### 🔴 ISSUE 2: Normal size lost through maximize → close → reopen → restore cycle (THE "OVERSIZED WINDOW" BUG)

**Severity:** High
**Test:** `test_non_maximized_size_preserved_through_maximize_cycle`
**Symptom:** After closing while maximized and reopening, restoring from maximized
leaves the window at the full screen size (2576x1408) instead of the pre-maximize
normal size (750x550).

**Evidence:**
- Set window to 750x550
- Maximized, then closed gracefully
- Saved state: `{"width": 2560, "height": 1369, "maximized": true}`
- Reopened → window appears maximized (correct)
- Restored → window stays at 2576x1408 (WRONG, should be ~750x550)

**Root cause:** `save_current_window_state()` saves `self.dimensions.pixel_width/height`
which are the CURRENT dimensions. When maximized, these are the maximized dimensions.
On restore, the window was CREATED at 2560x1369 before `window.maximize()` was called,
so Win32's `WINDOWPLACEMENT.rcNormalPosition` is set to the maximized size.

The save → restore flow:
1. **Save (maximized):** saves width=2560, height=1369, maximized=true
2. **Restore: create window:** creates at 2560x1369 (from saved width/height)
3. **Restore: maximize:** calls `window.maximize()` — the window was already large,
   Win32 records the 2560x1369 as the "normal" rect in WINDOWPLACEMENT
4. **User unmaximizes:** Win32 restores to rcNormalPosition = 2560x1369 → OVERSIZED

```
wezterm-gui/src/termwindow/mod.rs:2018-2019  ← saves current (maximized) dimensions
wezterm-gui/src/termwindow/mod.rs:856-857    ← restores those dimensions as window size
wezterm-gui/src/termwindow/mod.rs:944-946    ← then maximizes on top of already-large window
```

**Fix:** When saving while maximized, save the NORMAL (restored) dimensions, not the
current maximized dimensions. The proper approach:

```rust
fn save_current_window_state(&self) {
    // When maximized, we need the NORMAL (restored) position and size,
    // not the current maximized dimensions. Use WINDOWPLACEMENT.
    let (x, y, width, height) = if self.window_state.contains(WindowState::MAXIMIZED)
        || self.window_state.contains(WindowState::FULL_SCREEN)
    {
        // Get the normal rect from WINDOWPLACEMENT via the window handle
        // This is the rect the window will restore to when unmaximized
        self.window.as_ref()
            .and_then(|w| w.get_normal_placement())  // NEW METHOD NEEDED
            .unwrap_or((0, 0, self.dimensions.pixel_width, self.dimensions.pixel_height))
    } else {
        // Not maximized — save current position and client dimensions
        let (wx, wy) = self.window.as_ref()
            .and_then(|w| w.get_position())  // NEW METHOD NEEDED
            .unwrap_or((0, 0));
        (wx, wy, self.dimensions.pixel_width, self.dimensions.pixel_height)
    };

    let state = SavedWindowState {
        x, y, width, height,
        maximized: self.window_state.contains(WindowState::MAXIMIZED),
        fullscreen: self.window_state.contains(WindowState::FULL_SCREEN),
        monitor: self.current_screen_name.clone(),
    };
    save_window_state(&workspace, state);
}
```

This requires adding `get_position()` and `get_normal_placement()` methods to the
`WindowOps` trait (or the platform-specific `Window` struct) in `window/src/os/windows/window.rs`,
using `GetWindowRect` and `GetWindowPlacement` respectively.

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

### 🔴 ISSUE 3: Missing `WM_DPICHANGED` handler — window balloons on cross-monitor drag

**Severity:** High
**Test:** Manual (requires multi-monitor with different DPI — see `tests/ux/MANUAL_TESTS.md`)
**Symptom:** When dragging the window from one monitor to another with different DPI,
the drag outline shows a reasonable rectangle, but the final window size doesn't
match it — it balloons to a much larger size, sometimes exceeding the screen.

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

## Issue 5: Content Stretching / Multiple Redraws During Resize

**Severity:** Medium (visual quality)
**Symptom:** When the window is resized, the terminal content visually stretches
or scales before being redrawn at the correct dimensions. Multiple intermediate
redraws are visible, creating a jarring experience. The content appears to be
rendered at the old size then stretched to fill the new window dimensions before
the terminal recalculates and redraws at the correct cell count.

**Observed behavior:**
1. User starts resizing the window
2. Content is briefly STRETCHED (old content scaled to new dimensions)
3. Content redraws at new size (possibly multiple times)
4. Final correct rendering appears

**Expected behavior:**
- No content stretching — content should either clip or pad, never scale
- Single redraw at the final size, or smooth incremental resize

**Root cause:** The OpenGL/WebGPU rendering surface is resized by the window
system, which stretches the existing framebuffer content to fill the new surface
size. The terminal then recalculates cell dimensions and redraws, but there is
a visible frame (or several frames) where the stretched content is displayed.

**Code path:**
- `window/src/os/windows/window.rs:340-394` — `check_and_call_resize_if_needed()` dispatches resize event
- `wezterm-gui/src/termwindow/resize.rs:59-61` — WebGPU surface resized
- `wezterm-gui/src/termwindow/mod.rs:1117-1125` — paint deferred during resize (`is_repaint_pending`)
- `wezterm-gui/src/termwindow/mod.rs:1077-1088` — deferred paint executed after resize completes

**Possible fixes:**
- Clear the rendering surface to the background color BEFORE the terminal redraws,
  so the user sees a clean background instead of stretched content
- Use `WM_SIZING` to defer the surface resize until the user finishes dragging
- Implement DWM-aware composition to avoid showing intermediate frames

---

## Improvement Recommendations (Ranked)

### Priority 1: Fix the oversized-window-on-restore bug (Issue 2)
This is the most user-visible problem. Users who close WeezTerm while maximized
and then reopen it will get a permanently oversized window when they unmaximize.
**Files to modify:**
- `wezterm-gui/src/termwindow/mod.rs` — `save_current_window_state()`
- `window/src/os/windows/window.rs` — add `get_normal_placement()` using Win32 `GetWindowPlacement`
- `window/src/lib.rs` — add trait method to `WindowOps`

### Priority 2: Fix SSH mux connection stability (Issue 4)
SSH mux connections via `connect` with an isolated config drop after ~6 seconds.
This blocks all SSH mux resize/maximize testing and likely affects users.
**Files to investigate:**
- `wezterm-client/src/client.rs` — PDU decode error handling
- `wezterm-client/src/domain.rs` — ClientDomain connection lifecycle
- `codec/src/lib.rs` — codec version compatibility

### Priority 3: Add WM_DPICHANGED handler (Issue 3)
Window balloons when dragged between monitors with different DPI.
**Files to modify:**
- `window/src/os/windows/window.rs` — add `WM_DPICHANGED` case to `do_wnd_proc()`

### Priority 4: Fix window position persistence (Issue 1)
Users expect their window to reopen where they left it. Currently it always
opens at the OS-default position.
**Files to modify:**
- `wezterm-gui/src/termwindow/mod.rs` — populate x/y in `save_current_window_state()`
- `window/src/os/windows/window.rs` — add `get_position()` using Win32 `GetWindowRect`

### Priority 5: CI integration
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
