# --- weezterm remote features ---
"""Phase 2d UX tests for translucency / alpha-mode wiring.

These tests verify that the surface `alpha_mode` chosen at swap-chain
configuration time matches the user's translucency intent and the
active `RenderMode`:

  * Mode A (`wgpu_dcomp`) + translucency -> PreMultiplied
  * Mode A (`wgpu_dcomp`) + opaque       -> Opaque
  * Mode B (`wgpu_classic`)              -> legacy caps-driven cascade
                                            (PostMultiplied or PreMultiplied)
  * Mode C (`software_rdp`)              -> CPU path; no DXGI alpha_mode
                                            line, but must run clean

The contract is asserted via the `[render] surface ...` info line
emitted by `WebGpuState::new_impl()` after `surface.configure()`. If
that diagnostic line is renamed or removed, these tests must be
updated in lockstep.

DComp + translucency cannot be exercised under RDP: DComp swap chain
creation fails on WARP with `DXGI_ERROR_INVALID_CALL`. The
translucent_wgpu_dcomp test is skipped under RDP for that reason.
"""

import ctypes
import os
import re
import subprocess
import time
from pathlib import Path

import pytest

from helpers.app import WeezTermApp


pytestmark = pytest.mark.skipif(
    os.name != "nt", reason="Translucency wiring is Windows-only"
)


_SURFACE_LINE = re.compile(
    r"\[render\]\s+surface\s+mode=(\S+)\s+alpha_mode=(\S+)\s+"
    r"translucent=(true|false)\s+frame_latency=(\d+)\s+"
    r"present_mode=(\S+)\s+format=(\S+)"
)


def _is_rdp_session() -> bool:
    """Mirror `window::os::windows::is_running_in_rdp_session()` at test level."""
    try:
        SM_REMOTESESSION = 0x1000
        return bool(ctypes.windll.user32.GetSystemMetrics(SM_REMOTESESSION))
    except Exception:
        return False


def _spawn_capture(
    app: WeezTermApp,
    stderr_path: Path,
    extra_env: dict,
    run_seconds: float = 5.0,
) -> str:
    """Launch the app with extra env vars, capture stderr, return the text.

    Reuses the app fixture's `_build_env()` for full isolation
    (XDG_CONFIG_HOME / XDG_RUNTIME_DIR / scrubbed WEEZTERM_* vars).
    """
    env = app._build_env()
    env.update(extra_env)
    # The surface line is logged at info; force at least info-level so
    # release builds also emit it.
    env.setdefault("RUST_LOG", "wezterm_gui=info,window=info,info")

    cmd = [app.binary_path, "--config-file", app._config_file]
    with open(stderr_path, "wb") as err_f:
        proc = subprocess.Popen(
            cmd,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=err_f,
        )
        try:
            # Poll for the surface line as it streams; bail early once
            # seen so the test isn't pinned to a fixed sleep.
            deadline = time.time() + run_seconds + 15.0
            while time.time() < deadline:
                if proc.poll() is not None:
                    break
                try:
                    if stderr_path.exists() and stderr_path.stat().st_size > 0:
                        text = stderr_path.read_text(
                            encoding="utf-8", errors="replace"
                        )
                        if "[render] surface " in text or "[render] mode=" in text:
                            # Give the rest of the startup logs a moment
                            # so we capture both the `[render] mode=...`
                            # and the `[render] surface ...` lines.
                            time.sleep(0.8)
                            break
                except OSError:
                    pass
                time.sleep(0.2)

            # Then let it run for the requested duration so the renderer
            # exercises at least a few frames before we shut it down.
            time.sleep(max(0.0, run_seconds - 1.0))
            if proc.poll() is not None:
                pass  # captured below
        finally:
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)

    return stderr_path.read_text(encoding="utf-8", errors="replace")


def _assert_no_panic(captured: str) -> None:
    assert "panicked at" not in captured, (
        f"Process panicked.\n{captured[-2000:]}"
    )


def _make_app_with_config(config_lua: str) -> WeezTermApp:
    """Construct an isolated WeezTermApp with the given config_lua content."""
    a = WeezTermApp(config_lua=config_lua)
    return a


# --------------------------------------------------------------------------
# Configs
# --------------------------------------------------------------------------

_OPAQUE_CONFIG = """\
local wezterm = require 'wezterm'
return {
    front_end = "WebGpu",
    enable_tab_bar = true,
    initial_rows = 24,
    initial_cols = 80,
    window_decorations = "RESIZE|TITLE",
    animation_fps = 0,
    check_for_updates = false,
    audible_bell = "Disabled",
    window_background_opacity = 1.0,
    win32_system_backdrop = "Auto",
}
"""

_TRANSLUCENT_OPACITY_CONFIG = """\
local wezterm = require 'wezterm'
return {
    front_end = "WebGpu",
    enable_tab_bar = true,
    initial_rows = 24,
    initial_cols = 80,
    window_decorations = "RESIZE|TITLE",
    animation_fps = 0,
    check_for_updates = false,
    audible_bell = "Disabled",
    window_background_opacity = 0.85,
    win32_system_backdrop = "Auto",
}
"""

_TRANSLUCENT_MICA_CONFIG = """\
local wezterm = require 'wezterm'
return {
    front_end = "WebGpu",
    enable_tab_bar = true,
    initial_rows = 24,
    initial_cols = 80,
    window_decorations = "RESIZE|TITLE",
    animation_fps = 0,
    check_for_updates = false,
    audible_bell = "Disabled",
    window_background_opacity = 1.0,
    win32_system_backdrop = "Mica",
}
"""


# --------------------------------------------------------------------------
# Tests
# --------------------------------------------------------------------------


@pytest.mark.timeout(60)
def test_opaque_software_rdp_starts_clean(tmp_path):
    """Mode C + opaque: must start, render, and emit no DXGI surface line.

    SoftwareRdp uses the WARP+CPU path and never reaches the wgpu
    surface configuration code, so the `[render] surface ...` line is
    expected to be absent. The test instead asserts that the SoftwareRdp
    swap chain initialised and there are no Present1 errors.
    """
    app = _make_app_with_config(_OPAQUE_CONFIG)
    try:
        stderr_path = tmp_path / "stderr.txt"
        captured = _spawn_capture(
            app,
            stderr_path,
            extra_env={"WEEZTERM_RENDER_MODE": "software_rdp"},
            run_seconds=5.0,
        )

        print("\n  Captured stderr (last 1500 chars):")
        print(captured[-1500:])

        _assert_no_panic(captured)
        assert "[render] mode=software_rdp" in captured, (
            f"Expected mode=software_rdp diagnostic.\n{captured[-1500:]}"
        )
        assert "SoftwareRdp WARP swap chain initialised" in captured, (
            f"WARP swap chain never initialised.\n{captured[-1500:]}"
        )
        # The wgpu surface line is webgpu-only; SoftwareRdp must NOT emit it.
        assert "[render] surface " not in captured, (
            f"SoftwareRdp must not configure a wgpu surface, but found "
            f"'[render] surface ...' line.\n{captured[-1500:]}"
        )
        # No fall-back into the wgpu path.
        assert "falling back to OpenGL" not in captured, (
            f"SoftwareRdp construction fell back to OpenGL.\n{captured[-1500:]}"
        )
    finally:
        app.cleanup()


@pytest.mark.timeout(60)
def test_opaque_wgpu_classic_alpha_mode(tmp_path):
    """Mode B + opaque: alpha_mode comes from the legacy caps cascade.

    Under HWND swap chains (Mode B) DXGI only supports `DXGI_ALPHA_MODE_IGNORE`,
    which wgpu reports as `Opaque`. The legacy code path's cascade falls
    through to `Auto` when neither PostMultiplied nor PreMultiplied is
    reported by the adapter. The test asserts that the chosen mode is
    one of the legitimate caps-driven values, and that `translucent`
    is reported as `false`.
    """
    app = _make_app_with_config(_OPAQUE_CONFIG)
    try:
        stderr_path = tmp_path / "stderr.txt"
        captured = _spawn_capture(
            app,
            stderr_path,
            extra_env={"WEEZTERM_RENDER_MODE": "wgpu_classic"},
            run_seconds=5.0,
        )

        print("\n  Captured stderr (last 1500 chars):")
        print(captured[-1500:])

        _assert_no_panic(captured)

        match = _SURFACE_LINE.search(captured)
        assert match, (
            f"Did not find `[render] surface ...` diagnostic line.\n"
            f"{captured[-1500:]}"
        )
        mode, alpha_mode, translucent, frame_latency, _present, _format = match.groups()
        print(
            f"\n  Parsed: mode={mode} alpha_mode={alpha_mode} "
            f"translucent={translucent} frame_latency={frame_latency}"
        )

        assert mode == "wgpu_classic", (
            f"Expected mode=wgpu_classic, got {mode!r}"
        )
        assert translucent == "false", (
            f"Opaque config must report translucent=false, got {translucent!r}"
        )
        # Mode B uses the legacy cascade. Acceptable values on Windows are:
        #   PostMultiplied (reported by some drivers),
        #   PreMultiplied (reported by others),
        #   Auto (DXGI HWND fallback when neither is reported),
        #   Opaque (DXGI HWND swap chains commonly only report this).
        assert alpha_mode in {"PostMultiplied", "PreMultiplied", "Auto", "Opaque"}, (
            f"Mode B alpha_mode={alpha_mode!r} is not one of the legacy "
            f"caps-driven values."
        )
        # Mode B keeps the historical frame-latency of 2.
        assert frame_latency == "2", (
            f"Mode B must use desired_maximum_frame_latency=2, got "
            f"frame_latency={frame_latency!r}"
        )
    finally:
        app.cleanup()


@pytest.mark.timeout(60)
def test_translucent_wgpu_dcomp_alpha_mode(tmp_path):
    """Mode A + translucency: alpha_mode must be PreMultiplied.

    Skipped under RDP because DComp swap chain creation fails on WARP
    with `DXGI_ERROR_INVALID_CALL`. Outside RDP, this is the canonical
    happy-path translucency wiring.
    """
    if _is_rdp_session():
        pytest.skip(
            "DComp swap chain creation fails on WARP / RDP; this test is "
            "intentionally local-only. Run it on a non-RDP host with a "
            "real GPU."
        )

    app = _make_app_with_config(_TRANSLUCENT_OPACITY_CONFIG)
    try:
        stderr_path = tmp_path / "stderr.txt"
        captured = _spawn_capture(
            app,
            stderr_path,
            extra_env={"WEEZTERM_RENDER_MODE": "wgpu_dcomp"},
            run_seconds=5.0,
        )

        print("\n  Captured stderr (last 1500 chars):")
        print(captured[-1500:])

        _assert_no_panic(captured)

        match = _SURFACE_LINE.search(captured)
        assert match, (
            f"Did not find `[render] surface ...` diagnostic line.\n"
            f"{captured[-1500:]}"
        )
        mode, alpha_mode, translucent, frame_latency, _present, _format = match.groups()
        print(
            f"\n  Parsed: mode={mode} alpha_mode={alpha_mode} "
            f"translucent={translucent} frame_latency={frame_latency}"
        )

        assert mode == "wgpu_dcomp", (
            f"Expected mode=wgpu_dcomp, got {mode!r}"
        )
        assert translucent == "true", (
            f"Translucent config must report translucent=true, got "
            f"{translucent!r}"
        )
        assert alpha_mode == "PreMultiplied", (
            f"Mode A + translucency must use PreMultiplied, got "
            f"alpha_mode={alpha_mode!r}"
        )
        # Mode A uses frame-latency=1 (paired with the waitable handle).
        assert frame_latency == "1", (
            f"Mode A must use desired_maximum_frame_latency=1, got "
            f"frame_latency={frame_latency!r}"
        )
    finally:
        app.cleanup()


@pytest.mark.timeout(60)
def test_translucent_via_mica_backdrop(tmp_path):
    """Mode A + Mica backdrop: translucent=true even with opacity=1.0.

    Asserts that `window_uses_translucency()` correctly treats
    `win32_system_backdrop = "Mica"` as a translucency signal. Skipped
    under RDP for the same DComp reason as the opacity test.
    """
    if _is_rdp_session():
        pytest.skip(
            "DComp swap chain creation fails on WARP / RDP; this test is "
            "intentionally local-only."
        )

    app = _make_app_with_config(_TRANSLUCENT_MICA_CONFIG)
    try:
        stderr_path = tmp_path / "stderr.txt"
        captured = _spawn_capture(
            app,
            stderr_path,
            extra_env={"WEEZTERM_RENDER_MODE": "wgpu_dcomp"},
            run_seconds=5.0,
        )

        print("\n  Captured stderr (last 1500 chars):")
        print(captured[-1500:])

        _assert_no_panic(captured)

        match = _SURFACE_LINE.search(captured)
        assert match, (
            f"Did not find `[render] surface ...` diagnostic line.\n"
            f"{captured[-1500:]}"
        )
        mode, alpha_mode, translucent, _frame, _present, _format = match.groups()
        assert translucent == "true", (
            f"Mica backdrop must imply translucent=true, got {translucent!r}"
        )
        assert alpha_mode == "PreMultiplied", (
            f"Mica + Mode A must use PreMultiplied, got alpha_mode={alpha_mode!r}"
        )
    finally:
        app.cleanup()
# --- end weezterm remote features ---
