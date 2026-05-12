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

    # Phase 0 placeholder: must say auto-pending until Phase 1 wires up
    # actual mode selection. If you change this, update the test in p1.
    assert mode == "auto-pending", (
        f"Expected mode=auto-pending placeholder, got mode={mode!r}. "
        f"If you've shipped Phase 1, update this assertion."
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
# --- end weezterm remote features ---
