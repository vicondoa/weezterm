"""Startup time and initial rendering tests.

Tests that WeezTerm starts within acceptable time and renders
a fully-drawn window on startup.
"""

import time
import pytest
from helpers.app import WeezTermApp
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)
from helpers.timing import TimingResult

# Startup time threshold in milliseconds
STARTUP_THRESHOLD_MS = 8000  # 8 seconds — generous for cold start


@pytest.mark.startup
@pytest.mark.timeout(60)
class TestStartup:
    """Tests for application startup behavior."""

    def test_startup_time(self, app: WeezTermApp):
        """WeezTerm should show a visible window within the threshold."""
        startup_s = app.start(timeout=30)
        startup_ms = startup_s * 1000

        print(f"\n  Startup time: {startup_ms:.0f}ms")
        assert app.is_running, "WeezTerm should be running after start()"
        assert app.hwnd != 0, "WeezTerm should have a window handle"

        # Record but use generous threshold
        if startup_ms > STARTUP_THRESHOLD_MS:
            pytest.fail(
                f"Startup too slow: {startup_ms:.0f}ms "
                f"(threshold: {STARTUP_THRESHOLD_MS}ms)"
            )

    def test_startup_time_multiple_samples(self, app: WeezTermApp):
        """Measure startup time over multiple launches for reliability."""
        result = TimingResult()
        num_samples = 3

        for i in range(num_samples):
            startup_s = app.start(timeout=30)
            result.samples_ms.append(startup_s * 1000)
            time.sleep(1)
            app.stop()
            time.sleep(2)  # cool-down between launches

        print(f"\n  Startup timing: {result.summary()}")

        if result.p95_ms > STARTUP_THRESHOLD_MS:
            pytest.fail(
                f"Startup p95 too slow: {result.p95_ms:.0f}ms "
                f"(threshold: {STARTUP_THRESHOLD_MS}ms)\n"
                f"  All samples: {[f'{s:.0f}ms' for s in result.samples_ms]}"
            )

    def test_startup_window_fully_drawn(self, app: WeezTermApp):
        """After startup, the window should have no rendering artifacts.

        A terminal window is expected to be mostly dark (terminal background).
        We check for structural artifacts: tab bar not spanning full width,
        undrawn bands on right or bottom edges.
        """
        app.start(timeout=30)
        time.sleep(3.0)

        img = capture_window(app.hwnd)
        save_screenshot(img, "startup_fully_drawn")

        artifacts = detect_rendering_artifacts(img)
        print(f"\n  Artifacts found: {len(artifacts)}")
        for a in artifacts:
            print(f"    {a}")

        if artifacts:
            save_screenshot(img, "startup_fully_drawn", "ARTIFACT")
            pytest.fail(
                f"Startup window has rendering artifacts: {artifacts}"
            )

    def test_startup_no_transient_artifacts(self, app: WeezTermApp):
        """Capture multiple frames after startup to catch transient artifacts."""
        app.start(timeout=30)

        # Take screenshots at intervals
        frames = []
        for delay_s in [0.5, 1.0, 1.5, 2.0, 3.0]:
            time.sleep(0.5 if not frames else delay_s - frames[-1][0])
            img = capture_window(app.hwnd)
            artifacts = detect_rendering_artifacts(img)
            frames.append((delay_s, len(artifacts), artifacts))

        print("\n  Transient artifact timeline:")
        for t, count, arts in frames:
            print(f"    {t:.1f}s: {count} artifacts {arts if arts else ''}")

        # The final frame (3s after start) should have no artifacts
        final_artifacts = frames[-1][2]
        if final_artifacts:
            img = capture_window(app.hwnd)
            save_screenshot(img, "startup_transient", "FINAL_ARTIFACT")
            pytest.fail(
                f"Startup still has artifacts after 3s: {final_artifacts}"
            )
