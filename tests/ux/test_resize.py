"""Window resize behavior tests.

Tests that resizing the window (larger and smaller) produces a fully-drawn
result without persistent artifacts like black bands or split-screen areas.
"""

import time
import pytest
from helpers.app import WeezTermApp
from helpers.window_ops import (
    get_window_rect,
    set_window_rect,
    settle,
    is_visible,
)
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)
from helpers.timing import measure_operation


@pytest.mark.resize
@pytest.mark.timeout(90)
class TestResize:
    """Tests for window resize behavior."""

    def test_resize_smaller_no_artifacts(self, running_app: WeezTermApp):
        """Shrinking the window should not leave rendering artifacts."""
        hwnd = running_app.hwnd

        # Start at a known large size
        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(1.5)

        # Shrink significantly
        set_window_rect(hwnd, 100, 100, 600, 400)
        settle(2.0)  # generous settle for redraw

        img = capture_window(hwnd)
        save_screenshot(img, "resize_smaller")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after shrink: {artifacts}")
        if artifacts:
            save_screenshot(img, "resize_smaller", "ARTIFACT")
            pytest.fail(f"Resize smaller left rendering artifacts: {artifacts}")

    def test_resize_larger_no_artifacts(self, running_app: WeezTermApp):
        """Growing the window should redraw cleanly."""
        hwnd = running_app.hwnd

        # Start small
        set_window_rect(hwnd, 100, 100, 600, 400)
        settle(1.5)

        # Grow significantly
        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(2.0)

        img = capture_window(hwnd)
        save_screenshot(img, "resize_larger")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after grow: {artifacts}")
        if artifacts:
            save_screenshot(img, "resize_larger", "ARTIFACT")
            pytest.fail(f"Resize larger left rendering artifacts: {artifacts}")

    def test_rapid_resize_sequence(self, running_app: WeezTermApp):
        """Rapidly changing window size should not crash or leave permanent artifacts."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 100, 100, 1000, 800)
        settle(1.0)

        # Rapid size changes
        for width in range(1000, 400, -50):
            set_window_rect(hwnd, 100, 100, width, 600)
            time.sleep(0.05)  # very fast resize

        settle(2.5)  # generous settle after rapid changes

        # App should still be running
        assert running_app.is_running, "WeezTerm crashed during rapid resize"

        img = capture_window(hwnd)
        save_screenshot(img, "rapid_resize")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts after rapid resize: {artifacts}")
        if artifacts:
            save_screenshot(img, "rapid_resize", "ARTIFACT")
            pytest.fail(f"Rapid resize left rendering artifacts: {artifacts}")

    def test_resize_redraw_timing(self, running_app: WeezTermApp):
        """Measure how long it takes for the window to fully redraw after resize.

        Takes screenshots at intervals after resize to detect when artifacts clear.
        """
        hwnd = running_app.hwnd

        # Start large
        set_window_rect(hwnd, 100, 100, 1200, 900)
        settle(2.0)

        # Shrink (the problematic direction)
        set_window_rect(hwnd, 100, 100, 600, 400)

        # Take rapid screenshots to measure redraw time
        results = []
        for i in range(20):  # 20 captures over ~2 seconds
            time.sleep(0.1)
            img = capture_window(hwnd)
            black_pct = image_black_percentage(img)
            results.append((i * 100, black_pct))  # (ms_after_resize, black_pct)

        print("\n  Redraw timeline (ms -> black%):")
        for ms, pct in results:
            bar = "#" * int(pct / 2)
            print(f"    {ms:4d}ms: {pct:5.1f}% {bar}")

        # Save the first and last frames
        set_window_rect(hwnd, 100, 100, 600, 400)
        time.sleep(0.05)
        early = capture_window(hwnd)
        save_screenshot(early, "resize_timing", "early")
        time.sleep(2.0)
        late = capture_window(hwnd)
        save_screenshot(late, "resize_timing", "late")

        # The last frame should be artifact-free
        final_black = results[-1][1]
        print(f"\n  Final black percentage at 2000ms: {final_black:.1f}%")

    def test_resize_to_very_small(self, running_app: WeezTermApp):
        """Resizing to a very small window should not crash."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 100, 100, 200, 150)
        settle(2.0)

        assert running_app.is_running, "WeezTerm crashed when resized very small"

        rect = get_window_rect(hwnd)
        print(f"\n  Very small window rect: {rect}")

    def test_resize_to_very_large(self, running_app: WeezTermApp):
        """Resizing to a very large window should not crash or leave artifacts."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 0, 0, 2500, 1400)
        settle(2.0)

        assert running_app.is_running, "WeezTerm crashed when resized very large"

        img = capture_window(hwnd)
        save_screenshot(img, "resize_very_large")
        artifacts = detect_rendering_artifacts(img)
        if artifacts:
            save_screenshot(img, "resize_very_large", "ARTIFACT")
            pytest.fail(f"Very large resize left artifact: {artifacts}")
