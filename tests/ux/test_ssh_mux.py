"""SSH connection window tests.

Tests that window operations (resize, maximize, startup rendering) work
correctly when connected to a remote host via SSH.

Uses the `ssh` subcommand (direct SSH, no mux) for complete isolation —
the test creates its own SSH session with its own PTY, never touching
any existing mux sessions or workspaces.

Isolation:
- `ssh` subcommand creates a fresh SSH session (not mux)
- --config-file <temp> prevents connecting to local running GUI instances
- XDG_CONFIG_HOME / XDG_RUNTIME_DIR isolate local state
"""

import os
import subprocess
import time
from typing import Optional
import pytest
from helpers.app import WeezTermApp
from helpers.window_ops import (
    get_window_rect,
    set_window_rect,
    maximize,
    restore,
    is_maximized,
    set_foreground,
    settle,
    simulate_live_drag_resize,
)
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)
# --- weezterm remote features ---
from helpers.frame_capture import FrameCapture, summarize_frames
# --- end weezterm remote features ---
from helpers.timing import TimingResult


SSH_DOMAIN = "jvicondo-a7"
SSH_HOST = "jvicondo-a7"

# SSH connection thresholds
SSH_STARTUP_THRESHOLD_MS = 30000  # 30 seconds for SSH negotiation
SSH_SETTLE_TIME = 5.0  # seconds to wait after connection for rendering

# Path to TUI test script (deployed to remote via test)
TUI_TEST_SCRIPT = os.path.join(os.path.dirname(__file__), "tui_resize_test.py")


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSshMuxStartup:
    """Tests for SSH mux connection startup behavior."""

    def test_ssh_mux_connection_time(self, app: WeezTermApp):
        """SSH mux connection should establish within a reasonable time."""
        startup_s = app.start_ssh_mux(
            domain_name=SSH_DOMAIN,
            remote_address=SSH_HOST,
            timeout=60,
        )
        startup_ms = startup_s * 1000

        print(f"\n  SSH mux startup time: {startup_ms:.0f}ms")
        assert app.is_running, "WeezTerm should be running after SSH mux connect"
        assert app.hwnd != 0, "WeezTerm should have a window handle"

        if startup_ms > SSH_STARTUP_THRESHOLD_MS:
            pytest.fail(
                f"SSH mux startup too slow: {startup_ms:.0f}ms "
                f"(threshold: {SSH_STARTUP_THRESHOLD_MS}ms)"
            )

    def test_ssh_mux_window_fully_drawn(self, app: WeezTermApp):
        """After SSH mux connection, window should be fully rendered."""
        app.start_ssh_mux(
            domain_name=SSH_DOMAIN,
            remote_address=SSH_HOST,
            timeout=60,
        )
        # SSH mux needs more settle time for remote rendering
        time.sleep(SSH_SETTLE_TIME)

        if not app.is_running:
            stderr = app.last_stderr
            pytest.fail(
                f"SSH mux connection dropped after {SSH_SETTLE_TIME}s settle. "
                f"Stderr: {stderr[-500:] if stderr else '(empty)'}"
            )

        set_foreground(app.hwnd)
        img = capture_window(app.hwnd)
        save_screenshot(img, "ssh_mux_startup")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after SSH mux startup: {artifacts}")

        if artifacts:
            save_screenshot(img, "ssh_mux_startup", "ARTIFACT")
            pytest.fail(f"SSH mux startup has rendering artifacts: {artifacts}")

    def test_ssh_mux_startup_multiple_samples(self, app: WeezTermApp):
        """Measure SSH mux connection time over multiple launches."""
        result = TimingResult()
        num_samples = 2  # fewer samples since SSH is slow

        for i in range(num_samples):
            startup_s = app.start_ssh_mux(
                domain_name=SSH_DOMAIN,
                remote_address=SSH_HOST,
                timeout=60,
            )
            result.samples_ms.append(startup_s * 1000)
            time.sleep(2)
            app.stop()
            time.sleep(3)  # cool-down between SSH launches

        print(f"\n  SSH mux startup timing: {result.summary()}")


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSshMuxResize:
    """Tests for window resize behavior over SSH mux connection."""

    def test_resize_smaller_no_artifacts(self, ssh_mux_app: WeezTermApp):
        """Shrinking window over SSH mux should not leave artifacts."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(2.0)  # extra settle for remote redraw

        set_window_rect(hwnd, 100, 100, 600, 400)
        settle(3.0)  # more generous for SSH mux

        img = capture_window(hwnd)
        save_screenshot(img, "ssh_mux_resize_smaller")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after SSH mux shrink: {artifacts}")
        if artifacts:
            save_screenshot(img, "ssh_mux_resize_smaller", "ARTIFACT")
            pytest.fail(f"SSH mux resize smaller left artifacts: {artifacts}")

    def test_resize_larger_no_artifacts(self, ssh_mux_app: WeezTermApp):
        """Growing window over SSH mux should redraw cleanly."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 100, 100, 600, 400)
        settle(2.0)

        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(3.0)

        img = capture_window(hwnd)
        save_screenshot(img, "ssh_mux_resize_larger")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after SSH mux grow: {artifacts}")
        if artifacts:
            save_screenshot(img, "ssh_mux_resize_larger", "ARTIFACT")
            pytest.fail(f"SSH mux resize larger left artifacts: {artifacts}")

    def test_rapid_resize_over_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Rapid resize over SSH mux should not crash or leave permanent artifacts."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 100, 100, 1000, 800)
        settle(1.5)

        # Rapid resize — network latency may cause more visible artifacts
        for width in range(1000, 400, -100):
            set_window_rect(hwnd, 100, 100, width, 600)
            time.sleep(0.1)
            # Check if process died mid-resize
            if not ssh_mux_app.is_running:
                rc = ssh_mux_app._process.returncode if ssh_mux_app._process else "unknown"
                pytest.fail(
                    f"WeezTerm CRASHED during rapid SSH mux resize at width={width}. "
                    f"Exit code: {rc}"
                )

        settle(4.0)  # generous settle for remote to catch up

        if not ssh_mux_app.is_running:
            rc = ssh_mux_app._process.returncode if ssh_mux_app._process else "unknown"
            pytest.fail(f"WeezTerm crashed after rapid SSH mux resize. Exit code: {rc}")

        img = capture_window(hwnd)
        save_screenshot(img, "ssh_mux_rapid_resize")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after rapid SSH mux resize: {artifacts}")
        if artifacts:
            save_screenshot(img, "ssh_mux_rapid_resize", "ARTIFACT")
            pytest.fail(f"Rapid SSH mux resize left artifacts: {artifacts}")

    def test_resize_redraw_timing_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Measure redraw timing after resize over SSH mux.

        SSH mux may have higher latency — measure how long until
        the window is cleanly redrawn.
        """
        hwnd = ssh_mux_app.hwnd

        # Start large
        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(2.0)

        # Shrink
        set_window_rect(hwnd, 100, 100, 600, 400)

        # Take rapid screenshots to measure redraw time
        results = []
        for i in range(30):  # 30 captures over ~3 seconds
            time.sleep(0.1)
            img = capture_window(hwnd)
            artifacts = detect_rendering_artifacts(img)
            black_pct = image_black_percentage(img)
            results.append((i * 100, black_pct, len(artifacts)))

        print("\n  SSH mux redraw timeline (ms -> black% -> artifacts):")
        for ms, pct, arts in results:
            indicator = " *** ARTIFACT" if arts > 0 else ""
            print(f"    {ms:4d}ms: {pct:5.1f}% black, {arts} artifacts{indicator}")

        # Save early and late frames
        set_window_rect(hwnd, 100, 100, 600, 400)
        time.sleep(0.05)
        early = capture_window(hwnd)
        save_screenshot(early, "ssh_mux_resize_timing", "early")
        time.sleep(3.0)
        late = capture_window(hwnd)
        save_screenshot(late, "ssh_mux_resize_timing", "late")

    def test_resize_to_very_small_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Resizing to very small over SSH mux should not crash."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 100, 100, 200, 150)
        settle(3.0)

        if not ssh_mux_app.is_running:
            rc = ssh_mux_app._process.returncode if ssh_mux_app._process else "unknown"
            pytest.fail(f"WeezTerm CRASHED on very small resize over SSH mux. Exit code: {rc}")

        rect = get_window_rect(hwnd)
        print(f"\n  Very small SSH mux window: {rect}")

    def test_resize_to_very_large_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Resizing to very large over SSH mux should not crash or leave artifacts."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 0, 0, 2500, 1400)
        settle(3.0)

        if not ssh_mux_app.is_running:
            rc = ssh_mux_app._process.returncode if ssh_mux_app._process else "unknown"
            pytest.fail(f"WeezTerm CRASHED on very large resize over SSH mux. Exit code: {rc}")

        img = capture_window(hwnd)
        save_screenshot(img, "ssh_mux_resize_very_large")

        artifacts = detect_rendering_artifacts(img)
        if artifacts:
            save_screenshot(img, "ssh_mux_resize_very_large", "ARTIFACT")
            pytest.fail(f"Very large SSH mux resize left artifacts: {artifacts}")


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSshMuxMaximize:
    """Tests for maximize/unmaximize behavior over SSH mux."""

    def test_maximize_works_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Window should be maximizable over SSH mux."""
        hwnd = ssh_mux_app.hwnd

        assert not is_maximized(hwnd), "Should not start maximized"
        maximize(hwnd)
        settle(1.5)
        assert is_maximized(hwnd), "Should be maximized"

    def test_unmaximize_restores_size_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Restoring from maximized over SSH mux should preserve original size."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(2.0)
        original = get_window_rect(hwnd)
        print(f"\n  Original: {original}")

        maximize(hwnd)
        settle(1.5)

        restore(hwnd)
        settle(2.0)
        restored = get_window_rect(hwnd)
        print(f"  Restored: {restored}")

        width_diff = abs(restored.width - original.width)
        height_diff = abs(restored.height - original.height)
        print(f"  Width diff: {width_diff}px, Height diff: {height_diff}px")

        assert width_diff < 20, (
            f"SSH mux: width not restored: {original.width} -> {restored.width}"
        )
        assert height_diff < 20, (
            f"SSH mux: height not restored: {original.height} -> {restored.height}"
        )

    def test_unmaximize_fully_drawn_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """After unmaximize over SSH mux, window should be fully drawn."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(2.0)

        maximize(hwnd)
        settle(2.0)

        restore(hwnd)
        settle(3.0)  # extra settle for remote redraw

        img = capture_window(hwnd)
        save_screenshot(img, "ssh_mux_unmaximize")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after SSH mux unmaximize: {artifacts}")
        if artifacts:
            save_screenshot(img, "ssh_mux_unmaximize", "ARTIFACT")
            pytest.fail(f"SSH mux unmaximize left artifacts: {artifacts}")

    def test_maximize_restore_cycle_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Multiple maximize/restore cycles over SSH mux should be stable."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(2.0)
        original = get_window_rect(hwnd)

        for cycle in range(3):
            maximize(hwnd)
            settle(1.0)
            restore(hwnd)
            settle(1.0)
            current = get_window_rect(hwnd)
            print(f"\n  SSH mux cycle {cycle + 1}: {current}")

            assert abs(current.width - original.width) < 20, (
                f"Cycle {cycle + 1}: width drifted {original.width} -> {current.width}"
            )
            assert abs(current.height - original.height) < 20, (
                f"Cycle {cycle + 1}: height drifted {original.height} -> {current.height}"
            )


def _deploy_tui_script(host: str) -> str:
    """Copy the TUI resize test script to the remote host.
    Returns the remote path.
    """
    local_script = TUI_TEST_SCRIPT
    remote_path = "/tmp/tui_resize_test.py"
    # --- weezterm remote features ---
    # Clear the diag log so previous-run noise doesn't pollute the
    # current run's analysis.
    subprocess.run(
        ["ssh", host, "rm -f /tmp/tui_resize_diag.log"],
        check=False,
        timeout=30,
    )
    # --- end weezterm remote features ---
    subprocess.run(
        ["scp", "-q", local_script, f"{host}:{remote_path}"],
        check=True,
        timeout=10,
    )
    return remote_path


# --- weezterm remote features ---
def _fetch_remote_diag_log(host: str, dest_dir: str) -> Optional[str]:
    """scp the TUI's diag log back to the local test-results dir."""
    dest = os.path.join(dest_dir, "tui_resize_diag.log")
    try:
        subprocess.run(
            ["scp", "-q", f"{host}:/tmp/tui_resize_diag.log", dest],
            check=True,
            timeout=10,
        )
        return dest
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None
# --- end weezterm remote features ---


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSshTuiResize:
    """Tests using a TUI app on the remote side to validate resize behavior.

    Deploys a curses-based test app that draws borders, grid lines, and
    debug markers. After resize operations, captures screenshots to verify
    the TUI content is properly redrawn without cutoff or stretching.
    """

    @pytest.fixture(autouse=True)
    def _deploy_tui(self):
        """Deploy the TUI test script to the remote host."""
        try:
            self.remote_tui_path = _deploy_tui_script(SSH_HOST)
        except (subprocess.CalledProcessError, FileNotFoundError):
            pytest.skip("Cannot deploy TUI script to remote host via scp")

    def _start_ssh_with_tui(self, app: WeezTermApp, timeout=60):
        """Start SSH and run the TUI test script."""
        startup = app.start_ssh_mux(
            domain_name=SSH_DOMAIN,
            remote_address=SSH_HOST,
            timeout=timeout,
        )
        # Wait for SSH to connect
        time.sleep(3.0)

        # Type the command to run the TUI test script
        import ctypes
        user32 = ctypes.windll.user32
        user32.SetForegroundWindow(app.hwnd)
        time.sleep(0.5)

        # Send keystrokes to run the TUI app
        from helpers.window_ops import set_foreground
        set_foreground(app.hwnd)

        # Use SendInput to type the command
        cmd = f"python3 {self.remote_tui_path}\r"
        for ch in cmd:
            # Use WM_CHAR to send each character
            WM_CHAR = 0x0102
            if ch == '\r':
                user32.PostMessageW(app.hwnd, WM_CHAR, 13, 0)
            else:
                user32.PostMessageW(app.hwnd, WM_CHAR, ord(ch), 0)
            time.sleep(0.01)

        # Wait for TUI to start and draw
        time.sleep(3.0)
        return startup

    def test_tui_resize_borders_intact(self, ssh_mux_app: WeezTermApp):
        """After resize, the TUI border should span the full window."""
        hwnd = ssh_mux_app.hwnd
        set_foreground(hwnd)
        time.sleep(1.0)

        # Type command to run TUI
        import ctypes
        user32 = ctypes.windll.user32
        cmd = f"python3 {self.remote_tui_path}\r"
        for ch in cmd:
            WM_CHAR = 0x0102
            user32.PostMessageW(hwnd, WM_CHAR, 13 if ch == '\r' else ord(ch), 0)
            time.sleep(0.01)
        time.sleep(4.0)

        # Take screenshot with TUI at initial size
        img_before = capture_window(hwnd)
        save_screenshot(img_before, "tui_before_resize")

        # Resize
        set_window_rect(hwnd, 100, 100, 1200, 800)
        settle(3.0)

        img_after = capture_window(hwnd)
        save_screenshot(img_after, "tui_after_resize")

        # Check for artifacts
        artifacts = detect_rendering_artifacts(img_after)
        print(f"\n  TUI resize artifacts: {artifacts}")
        if artifacts:
            save_screenshot(img_after, "tui_resize", "ARTIFACT")
            pytest.fail(f"TUI resize left artifacts: {artifacts}")

    def test_tui_resize_smaller(self, ssh_mux_app: WeezTermApp):
        """Shrinking window with TUI should redraw correctly."""
        hwnd = ssh_mux_app.hwnd
        set_foreground(hwnd)
        time.sleep(1.0)

        import ctypes
        user32 = ctypes.windll.user32
        cmd = f"python3 {self.remote_tui_path}\r"
        for ch in cmd:
            WM_CHAR = 0x0102
            user32.PostMessageW(hwnd, WM_CHAR, 13 if ch == '\r' else ord(ch), 0)
            time.sleep(0.01)
        time.sleep(4.0)

        # Start large
        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(2.0)
        img_large = capture_window(hwnd)
        save_screenshot(img_large, "tui_large")

        # Shrink
        set_window_rect(hwnd, 100, 100, 600, 400)
        settle(3.0)
        img_small = capture_window(hwnd)
        save_screenshot(img_small, "tui_small")

        artifacts = detect_rendering_artifacts(img_small)
        print(f"\n  TUI shrink artifacts: {artifacts}")
        if artifacts:
            save_screenshot(img_small, "tui_shrink", "ARTIFACT")
            pytest.fail(f"TUI shrink left artifacts: {artifacts}")

    def test_tui_rapid_resize(self, ssh_mux_app: WeezTermApp):
        """Rapid resize with TUI running should not leave garbled content."""
        hwnd = ssh_mux_app.hwnd
        set_foreground(hwnd)
        time.sleep(1.0)

        import ctypes
        user32 = ctypes.windll.user32
        cmd = f"python3 {self.remote_tui_path}\r"
        for ch in cmd:
            WM_CHAR = 0x0102
            user32.PostMessageW(hwnd, WM_CHAR, 13 if ch == '\r' else ord(ch), 0)
            time.sleep(0.01)
        time.sleep(4.0)

        set_window_rect(hwnd, 100, 100, 1000, 800)
        settle(1.0)

        # Rapid resize
        for w in range(1000, 500, -50):
            set_window_rect(hwnd, 100, 100, w, 600)
            time.sleep(0.05)

        settle(4.0)  # let TUI redraw

        assert ssh_mux_app.is_running, "Crashed during rapid TUI resize"

        img = capture_window(hwnd)
        save_screenshot(img, "tui_rapid_resize")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  TUI rapid resize artifacts: {artifacts}")


# --- weezterm remote features ---
@pytest.mark.ssh_mux
@pytest.mark.timeout(300)
class TestSshTuiResizeDiagnostic:
    """Frame-by-frame diagnostic capture of SSH/TUI resize behavior.

    These tests do **not** assert pass/fail on rendering correctness —
    they exist purely to collect artifacts for human inspection so the
    user can see exactly what happens during a resize over SSH:

      - one PNG per ~16 ms continuously while the resize is in progress
      - a CSV manifest correlating frame index to elapsed time and
        live window dimensions
      - the GUI process stderr including the `[wm]` and `[resize]`
        debug logs
      - a metadata.txt summarizing the test parameters

    Output goes to ``tests/ux/test-results/diagnostic/<nodeid>/``.

    The captures target the two specific bugs the user reported:

      Bug 1 — "content stretches before snapping to the new size" during
              a window-border drag
      Bug 2 — "resizing the window LARGER causes the full-screen TUI app
              to first SHRINK to dimensions smaller than before, then
              jump to the right size" (two-phase resize)

    Each test deploys ``tui_resize_test.py`` to the remote host (a
    curses app drawing borders / grid / quadrant markers) so the bug
    pattern is visible in the captured frames as a cell-grid shift or a
    smaller-than-expected redrawn area.
    """

    @pytest.fixture(autouse=True)
    def _deploy_tui(self):
        try:
            self.remote_tui_path = _deploy_tui_script(SSH_HOST)
        except (subprocess.CalledProcessError, FileNotFoundError):
            pytest.skip("Cannot deploy TUI script to remote host via scp")

    def _start_tui(self, app: WeezTermApp):
        """Start the TUI app on the remote and wait for it to draw."""
        import ctypes

        user32 = ctypes.windll.user32
        set_foreground(app.hwnd)
        time.sleep(1.0)
        cmd = f"python3 {self.remote_tui_path}\r"
        for ch in cmd:
            WM_CHAR = 0x0102
            user32.PostMessageW(app.hwnd, WM_CHAR, 13 if ch == "\r" else ord(ch), 0)
            time.sleep(0.01)
        time.sleep(4.0)

    def _write_metadata(self, app: WeezTermApp, name: str, **fields):
        """Write a metadata.txt next to the captured frames."""
        path = os.path.join(app._diag_dir, "metadata.txt")
        with open(path, "w", encoding="utf-8") as f:
            f.write(f"test_name: {name}\n")
            for k, v in fields.items():
                f.write(f"{k}: {v}\n")

    def _slow_drag(self, hwnd: int, x: int, y: int, w_from: int, w_to: int,
                    h_from: int, h_to: int, steps: int, total_s: float, cap):
        """Simulate a gradual user-driven drag by interpolating between
        two sizes over `steps` linear steps spaced over `total_s`
        seconds. ``cap`` is the FrameCapture that's already running so
        the interpolation can call ``cap.note()`` between steps."""
        per_step = total_s / max(steps, 1)
        for i in range(steps + 1):
            t = i / max(steps, 1)
            w = int(round(w_from + (w_to - w_from) * t))
            h = int(round(h_from + (h_to - h_from) * t))
            cap.note(f"step {i}/{steps} -> set {w}x{h}")
            set_window_rect(hwnd, x, y, w, h)
            time.sleep(per_step)

    def test_diagnostic_slow_grow_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Slow drag from small to large — captures the BUG 2 sequence
        if it reproduces (large initial size NOT necessarily required —
        the TUI may shrink intermediately even on a smooth grow).

        Frames go to ``test-results/diagnostic/<nodeid>/frames/``.
        """
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        # Establish a known starting size and let the TUI redraw.
        set_window_rect(hwnd, 100, 100, 700, 500)
        settle(2.5)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.030)  # ~33 fps
        cap.start()
        cap.note("baseline (700x500)")
        time.sleep(0.3)

        cap.note("BEGIN slow grow 700x500 -> 1400x900 over 2.5s in 25 steps")
        self._slow_drag(hwnd, 100, 100, 700, 1400, 500, 900, steps=25, total_s=2.5, cap=cap)
        cap.note("END slow grow; settling 4s")
        time.sleep(4.0)
        cap.note("done")
        manifest = cap.stop()

        self._write_metadata(
            app,
            name="diagnostic_slow_grow_with_tui",
            from_size="700x500",
            to_size="1400x900",
            steps=25,
            total_s=2.5,
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")
        # Diagnostic only — no asserts.

    def test_diagnostic_slow_shrink_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Slow drag from large to small with TUI running."""
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        set_window_rect(hwnd, 100, 100, 1400, 900)
        settle(2.5)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.030)
        cap.start()
        cap.note("baseline (1400x900)")
        time.sleep(0.3)

        cap.note("BEGIN slow shrink 1400x900 -> 700x500 over 2.5s in 25 steps")
        self._slow_drag(hwnd, 100, 100, 1400, 700, 900, 500, steps=25, total_s=2.5, cap=cap)
        cap.note("END slow shrink; settling 4s")
        time.sleep(4.0)
        cap.note("done")
        manifest = cap.stop()

        self._write_metadata(
            app,
            name="diagnostic_slow_shrink_with_tui",
            from_size="1400x900",
            to_size="700x500",
            steps=25,
            total_s=2.5,
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")

    def test_diagnostic_one_step_grow_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Single programmatic resize larger — best repro for BUG 2.

        A one-shot SetWindowPos call from a small size to a large size.
        The user reports this causes the TUI to first SHRINK to a size
        smaller than the original, then jump to the correct (larger)
        size. The frames will show the intermediate shrink.
        """
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        set_window_rect(hwnd, 100, 100, 800, 500)
        settle(3.0)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.020)  # ~50 fps
        cap.start()
        cap.note("baseline (800x500); waiting 0.5s")
        time.sleep(0.5)

        cap.note("BEGIN one-step grow 800x500 -> 1600x1000")
        set_window_rect(hwnd, 100, 100, 1600, 1000)
        cap.note("END set_window_rect; settling 6s")
        time.sleep(6.0)
        cap.note("done")
        manifest = cap.stop()

        self._write_metadata(
            app,
            name="diagnostic_one_step_grow_with_tui",
            from_size="800x500",
            to_size="1600x1000",
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")

    def test_diagnostic_one_step_shrink_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Single programmatic resize smaller — captures BUG 1 (content
        stretching) if it reproduces."""
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        set_window_rect(hwnd, 100, 100, 1600, 1000)
        settle(3.0)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.020)
        cap.start()
        cap.note("baseline (1600x1000); waiting 0.5s")
        time.sleep(0.5)

        cap.note("BEGIN one-step shrink 1600x1000 -> 800x500")
        set_window_rect(hwnd, 100, 100, 800, 500)
        cap.note("END set_window_rect; settling 6s")
        time.sleep(6.0)
        cap.note("done")
        manifest = cap.stop()

        self._write_metadata(
            app,
            name="diagnostic_one_step_shrink_with_tui",
            from_size="1600x1000",
            to_size="800x500",
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")

    def test_diagnostic_step_resize_burst_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Sequence of distinct programmatic resizes spaced ~500ms apart
        with TUI running. Captures the full transient between each
        resize, including any double-resize artifact that may be
        triggered by WM_WINDOWPOSCHANGED + WM_SIZE both firing."""
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        set_window_rect(hwnd, 100, 100, 800, 500)
        settle(3.0)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.020)
        cap.start()
        cap.note("baseline (800x500); waiting 0.5s")
        time.sleep(0.5)

        sequence = [
            (1100, 700),
            (900, 600),
            (1400, 900),
            (1000, 700),
            (1600, 1000),
            (700, 500),
        ]
        for i, (w, h) in enumerate(sequence):
            cap.note(f"step {i}: set {w}x{h}")
            set_window_rect(hwnd, 100, 100, w, h)
            time.sleep(0.6)

        cap.note("END burst; settling 4s")
        time.sleep(4.0)
        cap.note("done")
        manifest = cap.stop()

        self._write_metadata(
            app,
            name="diagnostic_step_resize_burst_with_tui",
            sequence=";".join(f"{w}x{h}" for w, h in sequence),
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")

    def test_diagnostic_live_drag_grow_with_tui(self, ssh_mux_app_diagnostic: WeezTermApp):
        """Simulates a real interactive resize drag: WM_ENTERSIZEMOVE,
        many fast WM_SIZE events spaced ~20ms apart (typical for human
        drag), then WM_EXITSIZEMOVE.

        With the deferred / debounced surface.configure path, only a
        SINGLE configure should run at the END of the drag, instead
        of one per WM_SIZE step. Look in stderr.log for
        `webgpu surface.configure` lines — there should be ~1.

        Without the fix, the test would emit ~30 configures (one per
        WM_SIZE), each blocking the UI for 600-1700ms on the WARP
        virtual GPU."""
        app = ssh_mux_app_diagnostic
        hwnd = app.hwnd

        self._start_tui(app)
        set_window_rect(hwnd, 100, 100, 800, 500)
        settle(3.0)

        frames_dir = os.path.join(app._diag_dir, "frames")
        cap = FrameCapture(hwnd, frames_dir, interval_s=0.020)
        cap.start()
        cap.note("baseline (800x500); waiting 0.5s")
        time.sleep(0.5)

        cap.note("BEGIN live drag grow 800x500 -> 1600x1000 in 30 steps")
        simulate_live_drag_resize(
            hwnd, 100, 100,
            start_w=800, start_h=500,
            end_w=1600, end_h=1000,
            steps=30,
            step_delay_s=0.020,
        )
        cap.note("END live drag; settling 6s")
        time.sleep(6.0)
        cap.note("done")
        manifest = cap.stop()

        # --- weezterm remote features ---
        diag_path = _fetch_remote_diag_log(SSH_HOST, app._diag_dir)
        # --- end weezterm remote features ---

        self._write_metadata(
            app,
            name="diagnostic_live_drag_grow_with_tui",
            from_size="800x500",
            to_size="1600x1000",
            steps=30,
            frames_captured=cap.frames_captured,
            manifest=manifest,
        )
        print("\n  " + summarize_frames(frames_dir))
        print(f"  stderr log: {app.stderr_log_path}")
        # --- weezterm remote features ---
        if diag_path:
            print(f"  remote diag: {diag_path}")
        # --- end weezterm remote features ---
# --- end weezterm remote features ---
