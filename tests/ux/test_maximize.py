"""Maximize and unmaximize behavior tests.

Tests that maximizing and restoring a window preserves the original size
and does not leave the window in an oversized state.
"""

import time
import pytest
from helpers.app import WeezTermApp
from helpers.window_ops import (
    get_window_rect,
    set_window_rect,
    maximize,
    restore,
    is_maximized,
    get_normal_rect,
    settle,
)
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)


@pytest.mark.maximize
@pytest.mark.timeout(90)
class TestMaximize:
    """Tests for maximize/unmaximize behavior."""

    def test_maximize_works(self, running_app: WeezTermApp):
        """Window should be maximizable."""
        hwnd = running_app.hwnd

        assert not is_maximized(hwnd), "Window should not start maximized"
        maximize(hwnd)
        settle(1.0)
        assert is_maximized(hwnd), "Window should be maximized after maximize()"

    def test_unmaximize_restores_original_size(self, running_app: WeezTermApp):
        """Restoring from maximized should return to pre-maximize dimensions."""
        hwnd = running_app.hwnd

        # Set a known size first
        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(1.0)
        original = get_window_rect(hwnd)
        print(f"\n  Original rect: {original}")

        # Maximize then restore
        maximize(hwnd)
        settle(1.0)
        maximized = get_window_rect(hwnd)
        print(f"  Maximized rect: {maximized}")

        restore(hwnd)
        settle(1.0)
        restored = get_window_rect(hwnd)
        print(f"  Restored rect: {restored}")

        # Check that restored size matches original (within tolerance)
        width_diff = abs(restored.width - original.width)
        height_diff = abs(restored.height - original.height)
        print(f"  Width diff: {width_diff}px, Height diff: {height_diff}px")

        assert width_diff < 20, (
            f"Width not restored: original={original.width}, "
            f"restored={restored.width}, diff={width_diff}"
        )
        assert height_diff < 20, (
            f"Height not restored: original={original.height}, "
            f"restored={restored.height}, diff={height_diff}"
        )

    def test_unmaximize_not_oversized(self, running_app: WeezTermApp):
        """After unmaximize, window should NOT be larger than pre-maximize size."""
        hwnd = running_app.hwnd

        # Set a modest size
        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(1.0)
        original = get_window_rect(hwnd)

        # Maximize then restore
        maximize(hwnd)
        settle(1.0)
        restore(hwnd)
        settle(1.0)

        restored = get_window_rect(hwnd)
        print(f"\n  Original: {original}")
        print(f"  Restored: {restored}")

        # The restored window should not be bigger than the original
        tolerance = 20  # pixels
        assert restored.width <= original.width + tolerance, (
            f"Window is oversized after unmaximize: width {restored.width} > {original.width}"
        )
        assert restored.height <= original.height + tolerance, (
            f"Window is oversized after unmaximize: height {restored.height} > {original.height}"
        )

    def test_unmaximize_fully_drawn(self, running_app: WeezTermApp):
        """After unmaximize, the window should be fully drawn with no artifacts."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(1.5)

        maximize(hwnd)
        settle(1.5)

        restore(hwnd)
        settle(3.0)  # generous settle for redraw after restore

        # Retry capture a few times — the terminal may need an extra paint
        # cycle after the restore completes
        for attempt in range(3):
            img = capture_window(hwnd)
            save_screenshot(img, f"unmaximize_drawn_{attempt}")
            artifacts = detect_rendering_artifacts(img)
            if not artifacts:
                break
            settle(1.0)

        print(f"\n  Artifacts after unmaximize: {artifacts}")
        if artifacts:
            save_screenshot(img, "unmaximize_drawn", "ARTIFACT")
            pytest.fail(f"Unmaximize left rendering artifacts: {artifacts}")

    def test_maximize_restore_cycle(self, running_app: WeezTermApp):
        """Multiple maximize/restore cycles should maintain consistent size."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(1.0)
        original = get_window_rect(hwnd)

        sizes_after_restore = []
        for cycle in range(3):
            maximize(hwnd)
            settle(0.5)
            restore(hwnd)
            settle(0.5)
            current = get_window_rect(hwnd)
            sizes_after_restore.append(current)
            print(f"\n  Cycle {cycle + 1}: {current}")

        # All restored sizes should be close to original
        for i, rect in enumerate(sizes_after_restore):
            assert abs(rect.width - original.width) < 20, (
                f"Cycle {i + 1}: width drifted from {original.width} to {rect.width}"
            )
            assert abs(rect.height - original.height) < 20, (
                f"Cycle {i + 1}: height drifted from {original.height} to {rect.height}"
            )

    def test_normal_rect_preserved_while_maximized(self, running_app: WeezTermApp):
        """The 'normal' rect in WINDOWPLACEMENT should be correct while maximized."""
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 150, 800, 600)
        settle(1.0)
        original = get_window_rect(hwnd)

        maximize(hwnd)
        settle(1.0)

        # While maximized, the normal rect should still be the pre-maximize size
        normal = get_normal_rect(hwnd)
        print(f"\n  Original: {original}")
        print(f"  Normal rect while maximized: {normal}")

        assert abs(normal.width - original.width) < 30, (
            f"Normal rect width wrong: {normal.width} vs {original.width}"
        )
        assert abs(normal.height - original.height) < 30, (
            f"Normal rect height wrong: {normal.height} vs {original.height}"
        )

    def test_no_content_stretching_on_maximize(self, running_app: WeezTermApp):
        """Maximizing should not stretch terminal content.

        When going from a small window to maximized, the old content must not
        be visually stretched to fill the maximized dimensions. Instead, the
        surface should show the scheme background color in the new areas while
        the terminal redraws at the correct size.
        """
        import numpy as np
        hwnd = running_app.hwnd

        set_window_rect(hwnd, 200, 150, 600, 400)
        settle(2.0)

        # Maximize and capture frames immediately
        maximize(hwnd)

        stretched_frames = []
        for i in range(30):
            try:
                img = capture_window(hwnd)
                if img:
                    arr = np.array(img)
                    h, w = arr.shape[:2]
                    if w < 800 or h < 500:
                        continue

                    # Check expansion areas (right third, bottom third)
                    right_third = arr[:, w * 2 // 3:, :3]
                    bottom_third = arr[h * 2 // 3:, :, :3]
                    right_bright = float(np.mean(right_third))
                    bottom_bright = float(np.mean(bottom_third))

                    if right_bright > 25 or bottom_bright > 25:
                        save_screenshot(img, f"maximize_stretch_{i}")
                        stretched_frames.append(
                            (i, right_bright, bottom_bright)
                        )
            except Exception:
                pass
            time.sleep(0.016)

        print(f"\n  Frames with potential stretching: {len(stretched_frames)}")
        for idx, rb, bb in stretched_frames[:5]:
            print(f"    Frame {idx}: right_bright={rb:.1f}, bottom_bright={bb:.1f}")

        if stretched_frames:
            pytest.fail(
                f"Content stretching detected during maximize: "
                f"{len(stretched_frames)} frames"
            )
