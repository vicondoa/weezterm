"""Pytest fixtures for WeezTerm UX tests.

Provides an isolated WeezTermApp fixture that starts/stops the app
and cleans up temp directories even on test failure.
"""

import os
import pytest
from helpers.app import WeezTermApp


# Global timeout for all tests (seconds)
GLOBAL_TIMEOUT = 120


@pytest.fixture(scope="function")
def app():
    """Provide an isolated WeezTermApp instance.

    The app is NOT started automatically — tests call app.start() themselves
    so they can control timing. Cleanup always runs.
    """
    a = WeezTermApp()
    yield a
    a.cleanup()


@pytest.fixture(scope="function")
def running_app(app):
    """Provide a WeezTermApp that is already started and has a visible window.

    Convenience fixture for tests that don't need to measure startup.
    """
    app.start(timeout=30)
    # Let the window fully settle after startup
    import time
    time.sleep(2.0)
    # Ensure window is in foreground for visible testing
    from helpers.window_ops import set_foreground
    set_foreground(app.hwnd)
    yield app
    # cleanup handled by the `app` fixture


def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line("markers", "slow: marks tests as slow-running")
    config.addinivalue_line("markers", "startup: startup time tests")
    config.addinivalue_line("markers", "resize: window resize tests")
    config.addinivalue_line("markers", "maximize: maximize/unmaximize tests")
    config.addinivalue_line("markers", "dimensions: dimension persistence tests")
    config.addinivalue_line("markers", "ssh_mux: SSH mux connection tests")


# SSH mux test configuration
SSH_MUX_DOMAIN = "jvicondo-a7"
SSH_MUX_HOST = "jvicondo-a7"


@pytest.fixture(scope="function")
def ssh_mux_app(app):
    """Provide a WeezTermApp connected to jvicondo-a7 via SSH mux.

    Uses a unique workspace for isolation so the test doesn't
    interfere with any existing mux sessions on the remote host.
    Verifies the connection is stable before yielding.
    """
    import time
    app.start_ssh_mux(
        domain_name=SSH_MUX_DOMAIN,
        remote_address=SSH_MUX_HOST,
        timeout=60,
    )
    # SSH mux needs settle time — verify connection is stable for a full 10s
    import time
    time.sleep(3.0)

    # Check stability every second for 7 more seconds
    for i in range(7):
        time.sleep(1.0)
        if not app.is_running:
            stderr = app.last_stderr
            pytest.skip(
                f"SSH mux connection to {SSH_MUX_DOMAIN} dropped at t+{3+i+1}s. "
                f"This may be a conflict with an existing mux session. "
                f"Stderr: {stderr[-300:] if stderr else '(empty)'}"
            )

    from helpers.window_ops import set_foreground, get_window_rect
    rect = get_window_rect(app.hwnd)
    if rect.width == 0 or rect.height == 0:
        pytest.skip(
            f"SSH mux window has zero dimensions after settle — connection likely dropped"
        )

    set_foreground(app.hwnd)
    yield app
