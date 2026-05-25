# --- weezterm remote features ---
"""Cursor blink stutter test (p6 / Option 6A).

The dedicated cursor-blink thread (`wezterm-gui/src/cursor_blink_thread.rs`)
is supposed to keep the blink schedule firing even when the WindowProc
message loop is busy with WM_PAINT traffic from a heavy `cat`-style
workload that would otherwise starve the existing smol-Timer reschedule
in `wezterm-gui/src/termwindow/render/paint.rs`.

Reliably detecting cursor blink via screenshot diff requires:
  - Stable cursor location (no scrolling content overlapping it).
  - Pixel-level capture at a higher rate than the blink rate.
  - A way to distinguish cursor pixels from background.

The current UX harness doesn't ship with primitives for that, so the
automated assertion is skipped. The manual procedure (M8 in
`tests/ux/MANUAL_TESTS.md`) covers it for now.
"""
import pytest


@pytest.mark.skip(
    reason="Manual blink-rate verification; see tests/ux/MANUAL_TESTS.md M8 "
    "(no automated screenshot-diff primitive for blink detection yet)."
)
def test_blink_visible_under_scroll_load():
    """Placeholder; see manual test M8."""
    pass
# --- end weezterm remote features ---
