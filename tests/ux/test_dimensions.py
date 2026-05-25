"""Window dimension persistence tests.

Tests that window size, position, and maximized state are preserved
across application restarts via window-state.json.
"""

import json
import time
import os
import pytest
from helpers.app import WeezTermApp
from helpers.window_ops import (
    get_window_rect,
    set_window_rect,
    maximize,
    restore,
    is_maximized,
    settle,
)


@pytest.mark.dimensions
@pytest.mark.timeout(120)
class TestDimensionPersistence:
    """Tests for window state persistence across restarts."""

    def test_dimensions_preserved_on_restart(self, app: WeezTermApp):
        """Window should reopen at the same size after restart."""
        # Start and set a specific size
        app.start(timeout=30)
        settle(2.0)

        target_w, target_h = 900, 700
        set_window_rect(app.hwnd, 200, 150, target_w, target_h)
        settle(1.5)

        before = get_window_rect(app.hwnd)
        print(f"\n  Before stop: {before}")

        # Stop gracefully (allows window state to be saved)
        app.stop(timeout=5)
        settle(2.0)

        # Check if window-state.json was written
        state_file = app.window_state_file
        state_exists = os.path.exists(state_file)
        print(f"  Window state file exists: {state_exists}")
        if state_exists:
            with open(state_file) as f:
                state = json.load(f)
            print(f"  Saved state: {json.dumps(state, indent=2)}")

        # Restart
        app.start(timeout=30)
        settle(2.0)

        after = get_window_rect(app.hwnd)
        print(f"  After restart: {after}")

        width_diff = abs(after.width - target_w)
        height_diff = abs(after.height - target_h)
        print(f"  Width diff: {width_diff}px, Height diff: {height_diff}px")

        # Allow some tolerance for window chrome differences
        tolerance = 30
        if width_diff > tolerance:
            pytest.fail(
                f"Width not preserved: target={target_w}, got={after.width}, diff={width_diff}"
            )
        if height_diff > tolerance:
            pytest.fail(
                f"Height not preserved: target={target_h}, got={after.height}, diff={height_diff}"
            )

    def test_position_preserved_on_restart(self, app: WeezTermApp):
        """Window should reopen at the same screen position after restart."""
        app.start(timeout=30)
        settle(2.0)

        target_x, target_y = 250, 175
        set_window_rect(app.hwnd, target_x, target_y, 800, 600)
        settle(1.5)

        before = get_window_rect(app.hwnd)
        print(f"\n  Before stop: {before}")

        app.stop(timeout=5)
        settle(2.0)

        app.start(timeout=30)
        settle(2.0)

        after = get_window_rect(app.hwnd)
        print(f"  After restart: {after}")

        x_diff = abs(after.x - target_x)
        y_diff = abs(after.y - target_y)
        print(f"  X diff: {x_diff}px, Y diff: {y_diff}px")

        tolerance = 30
        if x_diff > tolerance:
            pytest.fail(
                f"X position not preserved: target={target_x}, got={after.x}, diff={x_diff}"
            )
        if y_diff > tolerance:
            pytest.fail(
                f"Y position not preserved: target={target_y}, got={after.y}, diff={y_diff}"
            )

    def test_maximized_state_preserved(self, app: WeezTermApp):
        """If closed while maximized, should reopen maximized."""
        app.start(timeout=30)
        settle(2.0)

        # Set a known normal size first, then maximize
        set_window_rect(app.hwnd, 200, 150, 800, 600)
        settle(1.0)
        maximize(app.hwnd)
        settle(1.0)

        assert is_maximized(app.hwnd), "Window should be maximized before stop"
        print(f"\n  Maximized before stop: {is_maximized(app.hwnd)}")

        app.stop(timeout=5)
        settle(2.0)

        # Check saved state
        state_file = app.window_state_file
        if os.path.exists(state_file):
            with open(state_file) as f:
                state = json.load(f)
            print(f"  Saved state: {json.dumps(state, indent=2)}")

        app.start(timeout=30)
        settle(2.0)

        maximized_after = is_maximized(app.hwnd)
        print(f"  Maximized after restart: {maximized_after}")

        if not maximized_after:
            pytest.fail("Maximized state was not preserved across restart")

    def test_window_state_file_written(self, app: WeezTermApp):
        """Verify that window-state.json is written on graceful close."""
        app.start(timeout=30)
        settle(2.0)

        # Move window to a specific position
        set_window_rect(app.hwnd, 300, 200, 850, 650)
        settle(1.5)

        app.stop(timeout=5)
        settle(1.0)

        state_file = app.window_state_file
        print(f"\n  State file path: {state_file}")
        print(f"  File exists: {os.path.exists(state_file)}")

        if os.path.exists(state_file):
            with open(state_file) as f:
                state = json.load(f)
            print(f"  Contents: {json.dumps(state, indent=2)}")

            # Verify the state has reasonable values
            # state is keyed by workspace name (usually "default")
            for workspace, ws_state in state.items():
                # --- weezterm remote features ---
                # Schema v2 stores dimensions inside `workspace_relative_rect`
                # (the pre-maximize / restored normal rect). Older schemas
                # had top-level width/height; we accept either for safety.
                rect = ws_state.get("workspace_relative_rect") or {}
                origin = rect.get("origin") or {}
                w = rect.get("width", ws_state.get("width", 0))
                h = rect.get("height", ws_state.get("height", 0))
                x = origin.get("x", ws_state.get("x", "?"))
                y = origin.get("y", ws_state.get("y", "?"))
                print(f"  Workspace '{workspace}':")
                print(f"    Size: {w}x{h}")
                print(f"    Position: ({x}, {y})")
                assert w > 0, "Saved width should be positive"
                assert h > 0, "Saved height should be positive"
                # --- end weezterm remote features ---
        else:
            pytest.fail("window-state.json was not written on graceful close")

    def test_non_maximized_size_preserved_through_maximize_cycle(self, app: WeezTermApp):
        """The normal (non-maximized) size should survive a maximize/close/reopen cycle.

        Schema-v2 invariant: we save WINDOWPLACEMENT.rcNormalPosition (the
        pre-maximize rect) plus a separate `maximized: true` flag, so the
        first un-maximize after restart must land at the original normal
        size, not at the maximized dimensions.
        """
        app.start(timeout=30)
        settle(2.0)

        # Set a specific normal size
        target_w, target_h = 750, 550
        set_window_rect(app.hwnd, 200, 150, target_w, target_h)
        settle(1.0)

        # Maximize, then close while maximized
        maximize(app.hwnd)
        settle(1.0)

        app.stop(timeout=5)
        settle(2.0)

        # Restart — should open maximized
        app.start(timeout=30)
        settle(2.0)

        # Restore — should go back to the original normal size
        restore(app.hwnd)
        settle(1.0)

        after = get_window_rect(app.hwnd)
        print(f"\n  Target: {target_w}x{target_h}")
        print(f"  After maximize->close->reopen->restore: {after}")

        tolerance = 30
        width_diff = abs(after.width - target_w)
        height_diff = abs(after.height - target_h)

        if width_diff > tolerance:
            pytest.fail(
                f"Normal width lost through maximize cycle: "
                f"target={target_w}, got={after.width}"
            )
        if height_diff > tolerance:
            pytest.fail(
                f"Normal height lost through maximize cycle: "
                f"target={target_h}, got={after.height}"
            )

    # ------------------------------------------------------------------
    # schema-v2 rebuild tests
    # ------------------------------------------------------------------
    #
    # These tests cover the failure modes the schema-v2 rewrite is meant
    # to make impossible. They write `window-state.json` directly into
    # the test app's isolated `XDG_CONFIG_HOME` so they don't need any
    # real multi-monitor / DPI setup.

    @staticmethod
    def _write_state_file(app: WeezTermApp, content: dict):
        """Write window-state.json into the isolated config dir."""
        os.makedirs(app.config_dir, exist_ok=True)
        with open(app.window_state_file, "w") as f:
            json.dump(content, f, indent=2)

    @staticmethod
    def _v2_state(
        x: int = 100,
        y: int = 100,
        w: int = 800,
        h: int = 600,
        dpi: int = 96,
        monitor_name: str = "primary",
        maximized: bool = False,
    ) -> dict:
        """Build a schema-v2 PersistedWindowState dict for one workspace."""
        return {
            "schema": 2,
            "workspace": "default",
            "monitor_name": monitor_name,
            "workspace_relative_rect": {
                "origin": {"x": x, "y": y},
                "width": w,
                "height": h,
            },
            "persistence_dpi": dpi,
            "maximized": maximized,
            "fullscreen": False,
            "saved_at_unix_secs": 0,
        }

    def test_restart_with_disconnected_monitor_falls_back_gracefully(
        self, app: WeezTermApp
    ):
        """Saved state references a monitor that doesn't exist anymore.

        The persistence module's fallback chain (name → overlap → primary →
        first) must place the window on a real monitor at sensible coords,
        rather than off-screen or crashing.
        """
        # Inject a state file with an unresolvable monitor name and
        # workspace-relative coords that fit any monitor.
        self._write_state_file(
            app,
            {
                "default": self._v2_state(
                    x=120,
                    y=140,
                    w=820,
                    h=620,
                    monitor_name="nonexistent-monitor-xyz-12345",
                )
            },
        )

        app.start(timeout=30)
        settle(2.0)

        rect = get_window_rect(app.hwnd)
        print(f"\n  After restart with disconnected monitor: {rect}")

        # The window must be visible on *some* monitor — width/height > 0
        # and the rect must intersect the primary monitor's work area.
        assert rect.width > 0, "Window must have non-zero width"
        assert rect.height > 0, "Window must have non-zero height"

        # Check that the dimensions roughly match what we asked for. Even
        # when fallback kicks in we expect the saved width/height to be
        # honored on the resolved monitor.
        tolerance = 60
        width_diff = abs(rect.width - 820)
        height_diff = abs(rect.height - 620)
        if width_diff > tolerance or height_diff > tolerance:
            pytest.fail(
                f"After disconnected-monitor fallback, expected ~820x620, "
                f"got {rect.width}x{rect.height}"
            )

    def test_restart_with_off_screen_rect_is_centered(self, app: WeezTermApp):
        """A saved rect that doesn't intersect any monitor must not place
        the window off-screen — restore_window() should center on a real
        monitor instead."""
        # Massively negative x means the rect lives way off-screen for
        # every reasonable monitor configuration.
        self._write_state_file(
            app,
            {
                "default": self._v2_state(
                    x=-100000,
                    y=-100000,
                    w=820,
                    h=620,
                    monitor_name="primary",
                )
            },
        )

        app.start(timeout=30)
        settle(2.0)

        rect = get_window_rect(app.hwnd)
        print(f"\n  After restart with off-screen rect: {rect}")

        # The window must be visible — i.e. its top-left must be inside
        # the visible virtual screen area. We can't predict the exact
        # monitor without enumerating monitors here, but a CenteredOnMonitor
        # result will leave x/y at non-extreme values.
        assert rect.width > 0 and rect.height > 0
        assert rect.x > -10000, (
            f"Window x={rect.x} is still wildly off-screen — off-screen "
            f"validation didn't trigger"
        )
        assert rect.y > -10000, (
            f"Window y={rect.y} is still wildly off-screen — off-screen "
            f"validation didn't trigger"
        )

    def test_restart_with_schema_v1_file_does_not_crash(self, app: WeezTermApp):
        """A schema-v1 (legacy) file must be migrated to defaults without
        crashing. The window appears with the config-driven default geometry
        rather than reusing the buggy v1 coords."""
        # Schema-v1 shape: flat x/y/width/height + monitor + monitor_position.
        # Crucially, no `schema` field at all.
        self._write_state_file(
            app,
            {
                "default": {
                    "x": 12345,
                    "y": 67890,
                    "width": 800,
                    "height": 600,
                    "maximized": False,
                    "fullscreen": False,
                    "monitor": "some-old-monitor",
                    "monitor_position": "top-left",
                }
            },
        )

        # Should not crash on startup. Window should appear at default geometry.
        app.start(timeout=30)
        settle(2.0)

        rect = get_window_rect(app.hwnd)
        print(f"\n  After restart with schema-v1 file: {rect}")

        # The buggy v1 coords (12345, 67890) must NOT be used.
        assert rect.x < 10000, (
            f"Schema-v1 coords leaked through: x={rect.x} — migration is broken"
        )
        assert rect.y < 10000, (
            f"Schema-v1 coords leaked through: y={rect.y} — migration is broken"
        )
        assert rect.width > 0 and rect.height > 0, (
            "Window should be visible after schema-v1 migration"
        )

    @pytest.mark.skip(
        reason="needs harness extension: simulating a different DPI requires "
        "changing the system display scale, which the test runner can't do "
        "in-process. Covered by Rust unit test "
        "`window_state_persistence::tests::restore_rescales_for_dpi_change`."
    )
    def test_restart_at_different_dpi_rescales_window(self, app: WeezTermApp):
        """Save state at one DPI, restart on a monitor with a different DPI;
        the persisted rect should be rescaled so the visible content area
        matches.

        TODO: requires a way to simulate the second monitor having a
        different effective DPI than what was captured. The Rust unit test
        covers the rescale math; this end-to-end test would exercise the
        Win32 placement path. Add when we have a way to drive
        `SetThreadDpiAwarenessContext` on the test process or to spoof
        `GetDpiForMonitor`.
        """
        pass
