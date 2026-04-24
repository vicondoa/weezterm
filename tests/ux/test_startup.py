"""Startup time and initial rendering tests.

Tests that WeezTerm starts within acceptable time and renders
a fully-drawn window on startup.
"""

import time
import pytest
import numpy as np
from helpers.app import WeezTermApp
from helpers.screenshot import (
    capture_window,
    detect_rendering_artifacts,
    image_black_percentage,
    save_screenshot,
)
from helpers.timing import TimingResult

# Startup time threshold in milliseconds
# Cold start on debug builds can be ~18s due to GPU init and shader compilation.
# Warm starts are typically <2s. Use generous threshold for the single-sample test.
STARTUP_THRESHOLD_MS = 30000  # 30 seconds — accommodates debug cold start


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

    def test_startup_no_white_flash(self, app: WeezTermApp):
        """No white flash should be visible during startup.

        Captures screenshots at 50ms intervals from the moment the window
        appears. Any frame with >5% white pixels indicates a white flash.
        """
        import subprocess
        from helpers.app import _find_windows_for_pid

        env = app._build_env()
        cmd = [app.binary_path, "--config-file", app._config_file]
        t0 = time.perf_counter()
        app._process = subprocess.Popen(
            cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )

        flash_frames = []
        hwnd = 0
        deadline = t0 + 20
        frame_idx = 0
        while time.perf_counter() < deadline:
            if app._process.poll() is not None:
                break
            wins = _find_windows_for_pid(app._process.pid)
            if wins and not hwnd:
                hwnd = wins[0]
                app._hwnd = hwnd
            if hwnd:
                try:
                    img = capture_window(hwnd)
                    if img:
                        arr = np.array(img)
                        white_mask = np.all(arr[:, :, :3] > 240, axis=2)
                        white_pct = float(np.mean(white_mask) * 100)
                        elapsed_ms = (time.perf_counter() - t0) * 1000
                        if white_pct > 5.0:
                            save_screenshot(img, f"white_flash_{frame_idx}")
                            flash_frames.append((elapsed_ms, white_pct))
                        frame_idx += 1
                except Exception:
                    pass
            time.sleep(0.05)

        print(f"\n  Total frames captured: {frame_idx}")
        print(f"  White flash frames (>5% white): {len(flash_frames)}")
        for ms, pct in flash_frames[:5]:
            print(f"    {ms:.0f}ms: {pct:.1f}% white")

        if flash_frames:
            worst = max(flash_frames, key=lambda x: x[1])
            pytest.fail(
                f"White flash detected during startup: {len(flash_frames)} frames, "
                f"worst={worst[1]:.1f}% white at {worst[0]:.0f}ms"
            )
