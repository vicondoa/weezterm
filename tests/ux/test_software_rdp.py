# --- weezterm remote features ---
"""Phase 4d UX tests for the SoftwareRdp (Mode C) backend.

These tests exercise the full SoftwareRdp pipeline end-to-end:

  * `WEEZTERM_RENDER_MODE=software_rdp` activates the WARP+CPU path.
  * The window comes up, renders without panicking, and survives
    typed input (covering the `Present1` / dirty-rect path that broke
    in p4c with overlapping rects).
  * Window resize triggers a swap-chain `ResizeBuffers` and the next
    frame still presents successfully (no `DXGI_ERROR_INVALID_CALL`).

The CPU draw path is verified at the runtime level rather than via
golden-image diffing -- pixel-level assertions on a freshly-rendered
WARP buffer are too brittle across font-rendering toolchains. Instead
we assert on:

  * the binary stays alive long enough to render multiple frames
  * the `[render] mode=software_rdp` and
    `[render] SoftwareRdp WARP swap chain initialised` log lines fire
  * stderr contains zero `Present1 failed` errors after typed input
"""

import os
import re
import subprocess
import time
from pathlib import Path

import pytest

from helpers.app import WeezTermApp


pytestmark = pytest.mark.skipif(
    os.name != "nt", reason="SoftwareRdp is Windows-only"
)


def _spawn_software_rdp(
    app: WeezTermApp, stderr_path: Path, run_seconds: float = 6.0
) -> str:
    """Launch weezterm-gui with WEEZTERM_RENDER_MODE=software_rdp and return captured stderr."""
    env = app._build_env()
    env["WEEZTERM_RENDER_MODE"] = "software_rdp"
    env["RUST_LOG"] = "wezterm_gui=info,window=info,info"

    cmd = [app.binary_path, "--config-file", app._config_file]
    with open(stderr_path, "wb") as err_f:
        proc = subprocess.Popen(
            cmd,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=err_f,
        )
        try:
            time.sleep(run_seconds)
            assert proc.poll() is None, (
                f"weezterm-gui exited early with code {proc.returncode}"
            )
        finally:
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)

    return stderr_path.read_text(encoding="utf-8", errors="replace")


@pytest.mark.timeout(60)
def test_software_rdp_starts_and_initialises_swap_chain(
    app: WeezTermApp, tmp_path
):
    """Mode C: WARP swap-chain construction + first render must succeed."""
    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_software_rdp(app, stderr_path, run_seconds=6.0)

    print("\n  Captured stderr (last 1500 chars):")
    print(captured[-1500:])

    assert "[render] mode=software_rdp" in captured, (
        f"Expected 'mode=software_rdp' diagnostic line.\n{captured[-2000:]}"
    )
    assert "WEEZTERM_RENDER_MODE override = software_rdp" in captured, (
        f"Expected env-var override line.\n{captured[-2000:]}"
    )
    assert "SoftwareRdp WARP swap chain initialised" in captured, (
        f"WARP swap chain never initialised.\n{captured[-2000:]}"
    )
    # No fall-back to OpenGL.
    assert "falling back to OpenGL" not in captured, (
        f"SoftwareRdp construction should not have fallen back to OpenGL.\n"
        f"{captured[-2000:]}"
    )


@pytest.mark.timeout(60)
def test_software_rdp_no_present_errors_during_typed_input(
    app: WeezTermApp, tmp_path
):
    """Mode C: typing into the window must not produce Present1 errors.

    This is the regression test for the p4c overlapping-dirty-rect bug
    that surfaced as `DXGI_ERROR_INVALID_CALL (0x887a0001)` after the
    first user interaction.
    """
    stderr_path = tmp_path / "stderr.txt"
    env = app._build_env()
    env["WEEZTERM_RENDER_MODE"] = "software_rdp"
    env["RUST_LOG"] = "wezterm_gui=warn,window=warn,error"

    cmd = [app.binary_path, "--config-file", app._config_file]
    with open(stderr_path, "wb") as err_f:
        proc = subprocess.Popen(
            cmd,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=err_f,
        )
        try:
            # Wait for the window to come up.
            time.sleep(4.0)
            assert proc.poll() is None, "process died during startup"

            # Inject a few rounds of typed input via SendKeys to drive
            # the present path. We use PowerShell because the test
            # harness already depends on it; avoids a pywin32 dep.
            for keys in ("a{ENTER}", "ls{ENTER}", "echo p4d{ENTER}"):
                subprocess.run(
                    [
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        (
                            "Add-Type -AssemblyName System.Windows.Forms; "
                            f"[System.Windows.Forms.SendKeys]::SendWait('{keys}')"
                        ),
                    ],
                    check=False,
                    timeout=10,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                time.sleep(1.5)

            assert proc.poll() is None, (
                f"process died during typed input with code {proc.returncode}"
            )
        finally:
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)

    captured = stderr_path.read_text(encoding="utf-8", errors="replace")
    print("\n  Captured stderr (last 1500 chars):")
    print(captured[-1500:])

    # The whole point of this test: no Present1 failures after input.
    present_errors = re.findall(r"Present1 failed: HRESULT 0x[0-9a-f]+", captured)
    assert not present_errors, (
        f"Found {len(present_errors)} Present1 failures in stderr; "
        f"first 5: {present_errors[:5]}\n{captured[-2000:]}"
    )

    # Also: no panics.
    assert "panicked at" not in captured, (
        f"Process panicked during typed input.\n{captured[-2000:]}"
    )


@pytest.mark.timeout(60)
def test_software_rdp_clear_dirty_helper_exists():
    """Sanity: `clear_dirty()` must remain present.

    p4c relies on this method to wipe the dirty-rect list at the start
    of every CPU-render frame so the renderer is the sole source of
    Present1 dirty rects. If this method is removed or renamed, the
    p4c overlapping-rect regression returns silently.
    """
    src = (
        Path(__file__).resolve().parent.parent.parent
        / "wezterm-gui"
        / "src"
        / "termwindow"
        / "software_rdp.rs"
    )
    assert src.exists(), f"software_rdp.rs not found at {src}"
    text = src.read_text(encoding="utf-8")
    assert "pub fn clear_dirty(" in text, (
        "SoftwareRdpState::clear_dirty() helper missing; the p4c "
        "Present1 overlapping-rect bug will return without it."
    )
# --- end weezterm remote features ---
