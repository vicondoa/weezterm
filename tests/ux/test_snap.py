"""Windows Snap resize tests for SSH mux connections.

Tests that snapping the window (simulating Win+Left/Right) correctly
resizes the terminal, especially over SSH mux where async RPC
introduces latency.
"""

import time
import pytest
from helpers.app import WeezTermApp
from helpers.window_ops import (
    get_window_rect,
    get_client_rect,
    set_window_rect,
    snap_left,
    snap_right,
    get_work_area,
    maximize,
    restore,
    settle,
    set_foreground,
)
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    save_screenshot,
)


@pytest.mark.resize
@pytest.mark.timeout(60)
class TestSnapLocal:
    """Tests for snap behavior with local terminal."""

    def test_snap_left_dimensions(self, running_app: WeezTermApp):
        """Snap left should resize window to left half of work area."""
        hwnd = running_app.hwnd
        wa = get_work_area()

        # Start at a custom size
        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(1.5)

        snap_left(hwnd)
        settle(2.0)

        rect = get_window_rect(hwnd)
        print(f"\n  Work area: {wa}")
        print(f"  After snap left: {rect}")

        # Width should be ~half the work area
        expected_w = wa.width // 2
        assert abs(rect.width - expected_w) < 20, (
            f"Snap left width {rect.width} should be ~{expected_w}"
        )
        # Height should be ~work area height
        assert abs(rect.height - wa.height) < 20, (
            f"Snap left height {rect.height} should be ~{wa.height}"
        )

    def test_snap_left_no_artifacts(self, running_app: WeezTermApp):
        """Snap left should produce a cleanly drawn window."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(1.5)

        snap_left(hwnd)

        # Retry capture — snap animation + terminal redraw may need time
        for attempt in range(3):
            settle(2.0)
            img = capture_window(hwnd)
            save_screenshot(img, f"snap_left_local_{attempt}")
            artifacts = detect_rendering_artifacts(img)
            if not artifacts:
                break

        print(f"\n  Artifacts after snap left: {artifacts}")
        if artifacts:
            save_screenshot(img, "snap_left_local", "ARTIFACT")
            pytest.fail(f"Snap left left rendering artifacts: {artifacts}")

    def test_snap_right_then_left(self, running_app: WeezTermApp):
        """Snapping right then left should produce clean results each time."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(1.5)

        snap_right(hwnd)
        settle(2.0)
        img_right = capture_window(hwnd)
        save_screenshot(img_right, "snap_right_local")

        snap_left(hwnd)
        settle(2.0)
        img_left = capture_window(hwnd)
        save_screenshot(img_left, "snap_left_after_right")

        artifacts = detect_rendering_artifacts(img_left)
        print(f"\n  Artifacts after snap left (from right): {artifacts}")
        if artifacts:
            save_screenshot(img_left, "snap_left_after_right", "ARTIFACT")
            pytest.fail(f"Snap left from right left artifacts: {artifacts}")


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSnapSshMux:
    """Tests for snap behavior over SSH mux connection.

    This is the main test class for the reported bug: snap over SSH mux
    causes incorrect terminal sizing where the vertical alignment gets
    messed up after an initial correct display.
    """

    def test_snap_left_ssh_mux_dimensions(self, ssh_mux_app: WeezTermApp):
        """Snap left over SSH mux should resize to correct dimensions.

        This tests the specific bug: after snap, the terminal should
        maintain the correct rows/cols matching the snapped window size.
        """
        hwnd = ssh_mux_app.hwnd
        wa = get_work_area()

        # Start at a custom size
        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(3.0)  # extra settle for remote

        # Get initial client rect for reference
        pre_snap = get_client_rect(hwnd)
        print(f"\n  Pre-snap client rect: {pre_snap}")

        # Snap to left half
        snap_left(hwnd)

        # Take multiple screenshots to catch the "revert" behavior
        results = []
        for i in range(20):
            time.sleep(0.25)
            rect = get_client_rect(hwnd)
            img = capture_window(hwnd)
            save_screenshot(img, f"snap_ssh_mux_t{i}")
            artifacts = detect_rendering_artifacts(img)
            results.append((i * 250, rect, len(artifacts)))

        print("\n  Snap timeline (ms -> client_rect -> artifacts):")
        for ms, rect, arts in results:
            indicator = " *** ARTIFACT" if arts > 0 else ""
            print(f"    {ms:5d}ms: {rect} artifacts={arts}{indicator}")

        # After 5 seconds, should be artifact-free
        final_img = capture_window(hwnd)
        save_screenshot(final_img, "snap_ssh_mux_final")
        final_artifacts = detect_rendering_artifacts(final_img)
        if final_artifacts:
            save_screenshot(final_img, "snap_ssh_mux_final", "ARTIFACT")
            pytest.fail(
                f"Snap left over SSH mux has artifacts after 5s settle: "
                f"{final_artifacts}"
            )

    def test_snap_left_size_stability(self, ssh_mux_app: WeezTermApp):
        """After snap, terminal size should not revert to pre-snap dimensions.

        Captures client rect every 250ms for 5s after snap to detect
        the reported bug where dimensions briefly correct then revert.
        """
        hwnd = ssh_mux_app.hwnd

        # Start at a known size
        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(3.0)
        pre_client = get_client_rect(hwnd)
        print(f"\n  Pre-snap client: {pre_client}")

        snap_left(hwnd)
        time.sleep(0.1)  # tiny delay for snap to take effect
        expected_client = get_client_rect(hwnd)
        print(f"  Expected post-snap client: {expected_client}")

        # Monitor for dimension revert over 5 seconds
        revert_detected = False
        revert_details = []
        for i in range(20):
            time.sleep(0.25)
            current = get_client_rect(hwnd)
            # Check if dimensions reverted toward pre-snap size
            if (abs(current.width - pre_client.width) < 20 and
                    abs(current.height - pre_client.height) < 20 and
                    abs(current.width - expected_client.width) > 50):
                revert_detected = True
                revert_details.append((i * 250, current))

        if revert_details:
            print("  DIMENSION REVERT DETECTED:")
            for ms, rect in revert_details:
                print(f"    {ms}ms: {rect}")
            pytest.fail(
                f"Terminal dimensions reverted after snap! "
                f"Expected ~{expected_client.width}x{expected_client.height}, "
                f"but reverted to pre-snap dims at: "
                f"{[f'{ms}ms' for ms, _ in revert_details]}"
            )

    def test_snap_left_no_artifacts_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Snap left over SSH mux should not leave rendering artifacts."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(3.0)

        snap_left(hwnd)
        settle(5.0)  # generous settle for SSH mux

        img = capture_window(hwnd)
        save_screenshot(img, "snap_left_ssh_mux")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after SSH mux snap left: {artifacts}")
        if artifacts:
            save_screenshot(img, "snap_left_ssh_mux", "ARTIFACT")
            pytest.fail(f"SSH mux snap left has artifacts: {artifacts}")

    def test_snap_right_then_restore_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Snap right then restore should return to original size over SSH mux."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(3.0)
        original = get_window_rect(hwnd)

        snap_right(hwnd)
        settle(3.0)
        snapped = get_window_rect(hwnd)
        print(f"\n  Original: {original}")
        print(f"  Snapped right: {snapped}")

        # Restore from snap
        restore(hwnd)
        settle(3.0)
        restored = get_window_rect(hwnd)
        print(f"  Restored: {restored}")

        img = capture_window(hwnd)
        save_screenshot(img, "snap_restore_ssh_mux")

        artifacts = detect_rendering_artifacts(img)
        if artifacts:
            save_screenshot(img, "snap_restore_ssh_mux", "ARTIFACT")
            pytest.fail(f"Snap→restore over SSH mux has artifacts: {artifacts}")

    def test_snap_resize_during_snap_ssh_mux(self, ssh_mux_app: WeezTermApp):
        """Manual resize after snap should fix dimensions (user-reported workaround)."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(3.0)

        snap_left(hwnd)
        settle(2.0)

        # Take screenshot during potential bad state
        img_bad = capture_window(hwnd)
        save_screenshot(img_bad, "snap_before_manual_resize")
        artifacts_before = detect_rendering_artifacts(img_bad)

        # Now do a small manual resize (the user's workaround)
        rect = get_window_rect(hwnd)
        set_window_rect(hwnd, rect.x, rect.y, rect.width + 10, rect.height)
        settle(3.0)

        img_good = capture_window(hwnd)
        save_screenshot(img_good, "snap_after_manual_resize")
        artifacts_after = detect_rendering_artifacts(img_good)

        print(f"\n  Artifacts before manual resize: {len(artifacts_before)}")
        print(f"  Artifacts after manual resize: {len(artifacts_after)}")

        if artifacts_before and not artifacts_after:
            pytest.fail(
                "Confirmed bug: snap has artifacts that manual resize fixes. "
                f"Before: {artifacts_before}"
            )


# Path to TUI test script (deployed to remote via test)
import os
TUI_TEST_SCRIPT = os.path.join(os.path.dirname(__file__), "tui_resize_test.py")
SSH_HOST = "jvicondo-a7"


def _deploy_tui_script(host: str) -> str:
    """Copy the TUI resize test script to the remote host."""
    import subprocess
    remote_path = "/tmp/tui_resize_test.py"
    subprocess.run(
        ["scp", "-q", TUI_TEST_SCRIPT, f"{host}:{remote_path}"],
        check=True,
        timeout=10,
    )
    return remote_path


def _type_command(hwnd: int, cmd: str):
    """Send keystrokes to the window to type a command."""
    import ctypes
    user32 = ctypes.windll.user32
    WM_CHAR = 0x0102
    for ch in cmd:
        user32.PostMessageW(hwnd, WM_CHAR, 13 if ch == '\r' else ord(ch), 0)
        time.sleep(0.01)


@pytest.mark.ssh_mux
@pytest.mark.timeout(180)
class TestSnapTuiSshMux:
    """Tests for snap with a fullscreen TUI app running over SSH mux.

    Deploys a curses-based test app that draws borders and grid lines,
    then performs snap operations and checks the TUI redraws correctly.
    This tests the specific user-reported scenario where snap causes
    vertical misalignment in TUI apps over mux connections.
    """

    @pytest.fixture(autouse=True)
    def _deploy_tui(self):
        """Deploy the TUI test script to the remote host."""
        import subprocess
        try:
            self.remote_tui_path = _deploy_tui_script(SSH_HOST)
        except (subprocess.CalledProcessError, FileNotFoundError):
            pytest.skip("Cannot deploy TUI script to remote host via scp")

    def _start_tui(self, app: WeezTermApp):
        """Launch the TUI test script in the SSH session."""
        hwnd = app.hwnd
        set_foreground(hwnd)
        time.sleep(1.0)
        _type_command(hwnd, f"python3 {self.remote_tui_path}\r")
        time.sleep(4.0)  # wait for TUI to draw

    def test_tui_snap_left_no_artifacts(self, ssh_mux_app: WeezTermApp):
        """Snap left with TUI running should redraw correctly."""
        hwnd = ssh_mux_app.hwnd

        # Start at a known size, launch TUI
        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(2.0)
        self._start_tui(ssh_mux_app)

        img_before = capture_window(hwnd)
        save_screenshot(img_before, "tui_snap_before")

        # Snap left
        snap_left(hwnd)
        settle(5.0)  # generous settle for remote TUI redraw

        img_after = capture_window(hwnd)
        save_screenshot(img_after, "tui_snap_left")

        artifacts = detect_rendering_artifacts(img_after)
        print(f"\n  TUI snap left artifacts: {artifacts}")
        if artifacts:
            save_screenshot(img_after, "tui_snap_left", "ARTIFACT")
            pytest.fail(f"TUI snap left has artifacts: {artifacts}")

    def test_tui_snap_dimensions_stable(self, ssh_mux_app: WeezTermApp):
        """After snap, TUI should show consistent dimensions over time.

        Captures multiple screenshots after snap to detect the reported
        bug where dimensions briefly correct then revert.
        """
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(2.0)
        self._start_tui(ssh_mux_app)

        snap_left(hwnd)

        # Capture screenshots over 5 seconds to detect instability
        screenshots = []
        for i in range(10):
            time.sleep(0.5)
            img = capture_window(hwnd)
            save_screenshot(img, f"tui_snap_stability_{i}")
            artifacts = detect_rendering_artifacts(img)
            screenshots.append((i * 500, len(artifacts)))

        print("\n  TUI snap stability timeline:")
        for ms, n_arts in screenshots:
            indicator = " *** ARTIFACT" if n_arts > 0 else ""
            print(f"    {ms:5d}ms: {n_arts} artifacts{indicator}")

        # After 4 seconds, should be artifact-free
        late_artifacts = [n for ms, n in screenshots if ms >= 4000 and n > 0]
        if late_artifacts:
            pytest.fail(
                f"TUI still has artifacts {len(late_artifacts)} samples "
                f"after 4s settle"
            )

    def test_tui_snap_left_right_cycle(self, ssh_mux_app: WeezTermApp):
        """Snap left then right with TUI should redraw correctly each time."""
        hwnd = ssh_mux_app.hwnd

        set_window_rect(hwnd, 200, 200, 800, 600)
        settle(2.0)
        self._start_tui(ssh_mux_app)

        # Snap left
        snap_left(hwnd)
        settle(4.0)

        img_left = capture_window(hwnd)
        save_screenshot(img_left, "tui_snap_cycle_left")
        artifacts_left = detect_rendering_artifacts(img_left)

        # Snap right
        snap_right(hwnd)
        settle(4.0)

        img_right = capture_window(hwnd)
        save_screenshot(img_right, "tui_snap_cycle_right")
        artifacts_right = detect_rendering_artifacts(img_right)

        print(f"\n  TUI snap left artifacts: {len(artifacts_left)}")
        print(f"  TUI snap right artifacts: {len(artifacts_right)}")

        if artifacts_left:
            pytest.fail(f"TUI snap left has artifacts: {artifacts_left}")
        if artifacts_right:
            pytest.fail(f"TUI snap right has artifacts: {artifacts_right}")
