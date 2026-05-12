# WeezTerm Manual UX Test Checklist

These tests require manual execution because they depend on hardware
configurations (multiple monitors, different DPIs) that can't be reliably
automated.

## Prerequisites

- WeezTerm built: `cargo build -p wezterm-gui`
- Two monitors with DIFFERENT resolutions/DPI scaling (e.g., 1080p @ 100% + 4K @ 150%)
- Know your monitor DPI settings: Settings → Display → Scale

---

## Test M1: Cross-Monitor Drag — Lower DPI → Higher DPI

**Setup:** Window on the lower-DPI monitor, sized to ~80x24 terminal cells.

**Steps:**
1. Note the window size (width × height in pixels) and terminal cells (rows × cols)
2. Drag the window to the higher-DPI monitor
3. Wait for the window to settle (2-3 seconds)

**Expected:**
- Window should maintain approximately the same PHYSICAL size (inches on screen)
- Terminal rows and cols should remain the same
- Window should NOT be larger than the monitor work area
- Window should not "balloon" beyond the drag outline

**Record:**
- [ ] Window stayed within monitor bounds: YES / NO
- [ ] Terminal rows/cols preserved: ___×___ → ___×___
- [ ] Window pixel size before: ___×___
- [ ] Window pixel size after: ___×___
- [ ] Qualitative: smooth / flickered / ballooned / clipped

---

## Test M2: Cross-Monitor Drag — Higher DPI → Lower DPI

**Setup:** Window on the higher-DPI monitor.

**Steps:**
1. Note window size and terminal cells
2. Drag to lower-DPI monitor
3. Wait for settle

**Expected:**
- Window should shrink proportionally (same physical size)
- Terminal rows/cols preserved
- No rendering artifacts

**Record:**
- [ ] Window stayed within monitor bounds: YES / NO
- [ ] Terminal rows/cols preserved: ___×___ → ___×___
- [ ] Window pixel size before: ___×___
- [ ] Window pixel size after: ___×___

---

## Test M3: Cross-Monitor Drag Outline vs Final Position

**Setup:** Window on either monitor, not maximized.

**Steps:**
1. Start dragging the title bar toward the other monitor
2. Observe the drag outline/shadow shown by Windows
3. Release the mouse on the target monitor
4. Compare final window geometry to the outline shown during drag

**Expected:**
- Final window size/position should match (or closely match) the drag outline
- Window should NOT jump to a completely different size after release

**Record:**
- [ ] Outline matches final position: YES / NO
- [ ] If NO, describe mismatch: _______________

---

## Test M4: Maximize on Monitor 1, Drag to Monitor 2

**Steps:**
1. Maximize on monitor 1
2. Drag the title bar (Windows auto-restores from maximized during drag)
3. Drop on monitor 2

**Expected:**
- Window restores to pre-maximize size before moving
- After drop, may rescale for new DPI but should not exceed monitor bounds

**Record:**
- [ ] Restored size reasonable: YES / NO
- [ ] Fits within target monitor: YES / NO

---

## Test M5: Rapid Cross-Monitor Bouncing

**Steps:**
1. Quickly drag the window back and forth between monitors 5 times
2. Let it settle on the original monitor

**Expected:**
- No crash
- Final size should be close to the starting size
- No accumulated drift

**Record:**
- [ ] Starting size: ___×___
- [ ] Final size: ___×___
- [ ] Drift: ___ pixels
- [ ] Crashed: YES / NO

---

## Known Issue: Window Balloons on Monitor Change

See the UX findings report for root cause analysis and fix recommendations.
The manual tests above (M1–M5) are designed to verify this behavior.

---

<!-- --- weezterm remote features --- -->
## Test M6: SoftwareRdp (Mode C) — Cold Start in RDP Session

**Setup:** Connect to the host via Remote Desktop. Confirm there is no
locally-attached GPU (the only adapters reported by `Get-PnpDevice -Class
Display` should be `Microsoft Basic Render Driver` / WARP / virtual GPUs).

**Steps:**
1. Make sure no `WEEZTERM_RENDER_MODE` env var is set:
   `Remove-Item Env:WEEZTERM_RENDER_MODE -ErrorAction SilentlyContinue`
2. Launch: `.\target\debug\weezterm-gui.exe`
3. Wait 3 seconds for the window to become visible
4. Type a few commands (`dir`, `cd`, `cls`)
5. Resize the window by dragging the bottom-right corner
6. Maximise and restore once

**Expected:**
- Stderr contains `[render] mode=software_rdp rdp=true` (Auto resolved correctly)
- Stderr does NOT contain `[render] WEEZTERM_RENDER_MODE override` (no env var)
- Stderr contains `SoftwareRdp WARP swap chain initialised (WxH)`
- Stderr does NOT contain any `Present1 failed: HRESULT 0x887a0001`
- Cursor blinks
- Text appears in the correct font/colour as you type
- Window resize redraws cleanly (no torn frames, no permanent artefacts)
- No crash

**Record:**
- [ ] mode=software_rdp logged: YES / NO
- [ ] No Present1 errors: YES / NO
- [ ] Text input visible: YES / NO
- [ ] Resize clean: YES / NO
- [ ] Maximise/restore clean: YES / NO

---

## Test M7: SoftwareRdp (Mode C) — Force-Override Smoke Test

**Setup:** Same machine as M6, but explicitly force the SoftwareRdp backend
even on hosts where Auto would not pick it.

**Steps:**
1. `$env:WEEZTERM_RENDER_MODE = 'software_rdp'`
2. Launch: `.\target\debug\weezterm-gui.exe`
3. Run `python tests\ux\test_software_rdp.py` (or
   `python -m pytest tests/ux/test_software_rdp.py -v -s`)
4. Verify all 3 automated tests pass
5. Manually exercise the window for 30 seconds: typed input, scrolling,
   resize, maximise/restore

**Expected:**
- All 3 automated tests pass
- Stderr contains `[render] WEEZTERM_RENDER_MODE override = software_rdp`
- Stderr contains `[render] mode=software_rdp`
- No `Present1 failed` lines, no panics
- Window remains responsive throughout

**Record:**
- [ ] Automated tests passed (3/3): ___/3
- [ ] Override log line present: YES / NO
- [ ] Smooth interactive use for 30s: YES / NO
<!-- --- end weezterm remote features --- -->

