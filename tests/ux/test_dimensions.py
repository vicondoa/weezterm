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
                print(f"  Workspace '{workspace}':")
                print(f"    Size: {ws_state.get('width', '?')}x{ws_state.get('height', '?')}")
                print(f"    Position: ({ws_state.get('x', '?')}, {ws_state.get('y', '?')})")
                assert ws_state.get("width", 0) > 0, "Saved width should be positive"
                assert ws_state.get("height", 0) > 0, "Saved height should be positive"
        else:
            pytest.fail("window-state.json was not written on graceful close")

    def test_non_maximized_size_preserved_through_maximize_cycle(self, app: WeezTermApp):
        """The normal (non-maximized) size should survive a maximize/close/reopen cycle."""
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
