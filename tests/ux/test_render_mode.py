# --- weezterm remote features ---
"""Phase 0 render-mode diagnostic tests.

These tests cover the additive logging + env-var override added in
Phase 0 of `docs/windows-rendering-design.md`. They do not assert on
which mode is selected — Phase 1 wires up actual mode resolution and
will extend this file with those assertions.

Each test launches `weezterm-gui.exe` with stderr captured to a temp
file (the binary's `env-bootstrap` logger writes to stderr at INFO
level by default) and greps for the expected `[render]` lines.
"""

import os
import re
import subprocess
import time
from pathlib import Path

import pytest

from helpers.app import WeezTermApp


def _spawn_with_stderr(app: WeezTermApp, stderr_path: Path, extra_env=None):
    """Launch weezterm-gui with stderr redirected to a file.

    Mirrors `WeezTermApp.start()` but routes stderr to a file instead of
    `DEVNULL` so we can grep the captured logs.
    """
    env = app._build_env()
    if extra_env:
        env.update(extra_env)

    cmd = [app.binary_path, "--config-file", app._config_file]
    with open(stderr_path, "wb") as err_f:
        proc = subprocess.Popen(
            cmd,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=err_f,
        )
        # Give the app enough time to emit startup logs.
        # The diagnostic line is logged synchronously inside try_new()
        # right after env-bootstrap setup_logger(), so a few seconds is
        # plenty even for cold debug starts.
        deadline = time.time() + 25
        while time.time() < deadline:
            if proc.poll() is not None:
                break
            # Check for the line as it streams; bail early once seen.
            try:
                if stderr_path.exists() and stderr_path.stat().st_size > 0:
                    text = stderr_path.read_text(encoding="utf-8", errors="replace")
                    if "[render] mode=" in text:
                        # Give it a small grace window for the override line
                        time.sleep(0.5)
                        break
            except OSError:
                pass
            time.sleep(0.2)

        # Close down cleanly
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)

    return stderr_path.read_text(encoding="utf-8", errors="replace")


@pytest.mark.timeout(60)
def test_startup_log_includes_render_diagnostics(app: WeezTermApp, tmp_path):
    """The `[render] mode=...` startup line must be present and parseable."""
    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_with_stderr(app, stderr_path)

    print("\n  Captured stderr (last 800 chars):")
    print(captured[-800:])

    # Match: [render] mode=<word> rdp=<bool> gpus=[...] win_build=<u32>
    pattern = re.compile(
        r"\[render\]\s+mode=(\S+)\s+rdp=(true|false)\s+gpus=\[(.*?)\]\s+win_build=(\d+)"
    )
    match = pattern.search(captured)
    assert match, (
        f"Could not find parseable [render] diagnostics line in stderr.\n"
        f"Last 1000 chars:\n{captured[-1000:]}"
    )

    mode, rdp, gpus, win_build = match.groups()
    print(f"\n  Parsed: mode={mode} rdp={rdp} gpus=[{gpus}] win_build={win_build}")

    # Phase 1: must report a concrete resolved mode (not the p0 placeholder).
    # The valid set is fixed by `RenderMode::as_str()` in
    # `window/src/render_mode.rs`; expand here when new modes are added.
    valid_modes = {"wgpu_dcomp", "wgpu_classic", "software_rdp"}
    assert mode in valid_modes, (
        f"Expected mode to be one of {sorted(valid_modes)}, got mode={mode!r}. "
        f"If you've added a new RenderMode variant, update this test."
    )


@pytest.mark.timeout(60)
def test_env_var_override_is_logged(app: WeezTermApp, tmp_path):
    """Setting WEEZTERM_RENDER_MODE must produce an override log line."""
    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_with_stderr(
        app,
        stderr_path,
        extra_env={"WEEZTERM_RENDER_MODE": "wgpu_classic"},
    )

    print("\n  Captured stderr (last 800 chars):")
    print(captured[-800:])

    expected = "[render] WEEZTERM_RENDER_MODE override = wgpu_classic"
    assert expected in captured, (
        f"Expected override line not found in stderr.\n"
        f"Looking for: {expected!r}\n"
        f"Last 1000 chars:\n{captured[-1000:]}"
    )


# --- weezterm remote features ---
# Phase 1 additions: verify that `auto` (explicit) and the auto-select fallback
# path resolve to the expected concrete mode for the current environment, and
# that an invalid env-var value falls back to auto with a logged warning.

_RENDER_LINE = re.compile(
    r"\[render\]\s+mode=(\S+)\s+rdp=(true|false)\s+gpus=\[(.*?)\]\s+win_build=(\d+)"
)


def _is_rdp_session() -> bool:
    """Match window::os::windows::is_running_in_rdp_session() at the test layer."""
    if os.name != "nt":
        return False
    try:
        import ctypes

        SM_REMOTESESSION = 0x1000
        return bool(ctypes.windll.user32.GetSystemMetrics(SM_REMOTESESSION))
    except Exception:
        return False


def _windows_build_number() -> int:
    if os.name != "nt":
        return 0
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        ) as key:
            value, _ = winreg.QueryValueEx(key, "CurrentBuildNumber")
            return int(value)
    except Exception:
        return 0


@pytest.mark.timeout(60)
def test_auto_resolves_to_software_rdp_in_rdp(app: WeezTermApp, tmp_path):
    """In an RDP session, `WEEZTERM_RENDER_MODE=auto` must pick software_rdp."""
    if not _is_rdp_session():
        pytest.skip("not running in an RDP session")

    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_with_stderr(
        app,
        stderr_path,
        extra_env={"WEEZTERM_RENDER_MODE": "auto"},
    )

    print("\n  Captured stderr (last 800 chars):")
    print(captured[-800:])

    match = _RENDER_LINE.search(captured)
    assert match, f"No [render] diagnostics line found.\n{captured[-1000:]}"
    mode = match.group(1)
    assert mode == "software_rdp", (
        f"Expected mode=software_rdp under RDP with WEEZTERM_RENDER_MODE=auto, "
        f"got mode={mode!r}.\n{captured[-1000:]}"
    )


@pytest.mark.timeout(60)
def test_auto_resolves_to_wgpu_locally(app: WeezTermApp, tmp_path):
    """Outside RDP, `auto` must pick wgpu_dcomp (build >= 19041) or wgpu_classic."""
    if _is_rdp_session():
        pytest.skip("running in an RDP session; this test is for local sessions only")

    # Cross-check via the env.txt fixture written by conftest.py.
    env_path = Path(__file__).resolve().parent / "test-results" / "env.txt"
    if env_path.exists():
        env_text = env_path.read_text(encoding="utf-8")
        assert "rdp=false" in env_text, (
            f"conftest.py recorded RDP-true but ctypes says false; environment is inconsistent.\n"
            f"env.txt:\n{env_text}"
        )

    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_with_stderr(
        app,
        stderr_path,
        extra_env={"WEEZTERM_RENDER_MODE": "auto"},
    )

    print("\n  Captured stderr (last 800 chars):")
    print(captured[-800:])

    match = _RENDER_LINE.search(captured)
    assert match, f"No [render] diagnostics line found.\n{captured[-1000:]}"
    mode = match.group(1)

    expected = "wgpu_dcomp" if _windows_build_number() >= 19041 else "wgpu_classic"
    assert mode == expected, (
        f"Expected mode={expected} (build={_windows_build_number()}), got mode={mode!r}. "
        f"NOTE: only_virtual_gpus_available() may force software_rdp on systems with "
        f"only Microsoft Basic Display / Hyper-V Video adapters.\n{captured[-1000:]}"
    )


@pytest.mark.timeout(60)
def test_invalid_env_var_falls_back_to_auto(app: WeezTermApp, tmp_path):
    """An unrecognized WEEZTERM_RENDER_MODE must warn and fall back to auto."""
    stderr_path = tmp_path / "stderr.txt"
    captured = _spawn_with_stderr(
        app,
        stderr_path,
        extra_env={"WEEZTERM_RENDER_MODE": "nonsense"},
    )

    print("\n  Captured stderr (last 800 chars):")
    print(captured[-800:])

    # Warning emitted from window::diagnostics::render_mode_override().
    assert "WEEZTERM_RENDER_MODE='nonsense' not recognized" in captured, (
        f"Expected warning for invalid env-var not found.\n{captured[-1000:]}"
    )

    # Falls back to auto-select; mode must be a real resolved variant.
    match = _RENDER_LINE.search(captured)
    assert match, f"No [render] diagnostics line found.\n{captured[-1000:]}"
    mode = match.group(1)
    assert mode in {"wgpu_dcomp", "wgpu_classic", "software_rdp"}, (
        f"Expected fallback to a real RenderMode, got mode={mode!r}.\n{captured[-1000:]}"
    )
    assert mode != "auto-pending", (
        f"Got the p0 placeholder string; the startup log was not updated for p1.\n"
        f"{captured[-1000:]}"
    )
# --- end weezterm remote features ---
