# --- weezterm remote features ---
"""Informational microbenchmark for Mode A (`WgpuDComp`) frame latency.

This is a *placeholder* file added in Phase 3 of the Windows rendering
implementation (see `docs/windows-rendering-design.md` §6 Phase 3 and
the project plan). The actual measurement code is intentionally not
implemented yet because:

  * Mode A only activates on a non-RDP Windows host with a real GPU
    (LLVMpipe/WARP-only environments degrade to Mode C). The dev box
    used during Phase 3 is an Azure VM accessed via RDP and cannot
    exercise Mode A meaningfully.
  * Measuring submit-to-visible-pixel latency requires either an
    external high-FPS capture rig or a software loop that screenshots
    via `mss`/`pywin32` and diffs against a known-good baseline. The
    accuracy of the latter on a virtualised display is poor.

When this is filled in, the test should:

  1. Launch `weezterm-gui.exe` with `WEEZTERM_RENDER_MODE=wgpu_dcomp`,
     reusing the isolation harness in `conftest.py`.
  2. Print a known glyph sequence into the terminal and capture
     pre-/post-print screenshots with monotonic timestamps.
  3. Assert that median submit-to-visible-pixel latency over N frames
     is <= 1 frame interval (~16.6 ms at 60 Hz) plus a generous
     tolerance (e.g. 25 ms).

Until then the test is skipped unconditionally so it does not inflate
CI runtime or produce spurious failures on environments that cannot
satisfy its preconditions.
"""

import pytest

pytestmark = pytest.mark.skip(
    reason=(
        "manual microbenchmark; requires non-RDP Win11 host with a real "
        "GPU. See module docstring for the implementation plan."
    )
)


def test_mode_a_frame_latency_le_one_frame():
    """Submit-to-visible-pixel latency under Mode A should be <= 1 frame."""
    # Intentionally unimplemented; see module docstring.
    raise NotImplementedError(
        "Phase 3 frame-latency microbenchmark is a placeholder. "
        "See `docs/windows-rendering-design.md` §6 Phase 3."
    )
# --- end weezterm remote features ---
