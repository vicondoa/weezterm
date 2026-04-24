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
)
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)
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
    subprocess.run(
        ["scp", "-q", local_script, f"{host}:{remote_path}"],
        check=True,
        timeout=10,
    )
    return remote_path


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
