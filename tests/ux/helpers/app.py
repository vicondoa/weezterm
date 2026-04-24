"""WeezTermApp: manages WeezTerm process lifecycle for UX testing.

Provides full process isolation via --config-file, XDG_CONFIG_HOME, and
XDG_RUNTIME_DIR so that tests never interfere with other running instances.
"""

import ctypes
import ctypes.wintypes
import os
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

# Win32 constants
PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_SYNCHRONIZE = 0x00100000
WAIT_TIMEOUT = 0x00000102
INFINITE = 0xFFFFFFFF
GW_OWNER = 4
GA_ROOTOWNER = 3

# Callback type for EnumWindows
WNDENUMPROC = ctypes.WINFUNCTYPE(
    ctypes.wintypes.BOOL,
    ctypes.wintypes.HWND,
    ctypes.wintypes.LPARAM,
)

# Default test config Lua
TEST_CONFIG_LUA = """\
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
    -- Use default shell (cmd.exe / powershell on Windows)
}
"""

# Config for SSH mux tests — includes the SSH domain.
# NOTE: Do NOT use --workspace with `connect` — there is a bug where
# non-default workspaces cause the mux connection to drop after ~6-8s.
SSH_MUX_CONFIG_LUA = """\
local wezterm = require 'wezterm'
return {{
    front_end = "WebGpu",
    enable_tab_bar = true,
    initial_rows = 24,
    initial_cols = 80,
    window_decorations = "RESIZE|TITLE",
    animation_fps = 0,
    check_for_updates = false,
    audible_bell = "Disabled",
    ssh_domains = {{
        {{
            name = "{domain_name}",
            remote_address = "{remote_address}",
            multiplexing = "WezTerm",
            remote_install_binaries_dir = [[C:\\src\\weezterm\\target\\cross-pkg\\linux-x86_64]],
        }},
    }},
}}
"""


def _find_binary() -> str:
    """Find the weezterm-gui binary."""
    # Check env var first
    env_path = os.environ.get("WEEZTERM_BINARY")
    if env_path and os.path.isfile(env_path):
        return env_path

    # Check cargo target directories
    repo_root = Path(__file__).resolve().parents[3]  # tests/ux/helpers -> repo root
    candidates = [
        repo_root / "target" / "debug" / "weezterm-gui.exe",
        repo_root / "target" / "release" / "weezterm-gui.exe",
    ]
    for c in candidates:
        if c.is_file():
            return str(c)

    raise FileNotFoundError(
        "Cannot find weezterm-gui.exe. Set WEEZTERM_BINARY env var or build with "
        "'cargo build -p wezterm-gui'"
    )


def _find_user_config() -> Optional[str]:
    """Find the user's actual wezterm.lua config file."""
    home = Path.home()
    candidates = [
        home / ".config" / "weezterm" / "wezterm.lua",
        home / ".config" / "wezterm" / "wezterm.lua",
        home / ".wezterm.lua",
        home / ".weezterm.lua",
    ]
    for c in candidates:
        if c.is_file():
            return str(c)
    return None


def _find_windows_for_pid(pid: int) -> list[int]:
    """Find all top-level window handles owned by the given PID."""
    results = []

    def callback(hwnd, _lparam):
        if not user32.IsWindowVisible(hwnd):
            return True
        window_pid = ctypes.wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(window_pid))
        if window_pid.value == pid:
            # Only top-level windows (no owner)
            owner = user32.GetWindow(hwnd, GW_OWNER)
            if not owner:
                results.append(hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(callback), 0)
    return results


@dataclass
class WeezTermApp:
    """Manages an isolated WeezTerm instance for testing."""

    binary_path: str = ""
    config_lua: str = TEST_CONFIG_LUA
    _temp_config: str = ""
    _temp_runtime: str = ""
    _config_file: str = ""
    _process: Optional[subprocess.Popen] = field(default=None, repr=False)
    _hwnd: int = 0
    _startup_time_s: float = 0.0
    _last_stderr: str = field(default="", repr=False)

    def __post_init__(self):
        if not self.binary_path:
            self.binary_path = _find_binary()
        self._temp_config = tempfile.mkdtemp(prefix="weezterm-ux-test-config-")
        self._temp_runtime = tempfile.mkdtemp(prefix="weezterm-ux-test-runtime-")
        # Config file goes under XDG_CONFIG_HOME/weezterm/wezterm.lua
        config_dir = os.path.join(self._temp_config, "weezterm")
        os.makedirs(config_dir, exist_ok=True)
        self._config_file = os.path.join(config_dir, "wezterm.lua")
        with open(self._config_file, "w") as f:
            f.write(self.config_lua)

    def _build_env(self) -> dict:
        """Build an isolated environment for the test process."""
        env = os.environ.copy()
        env["XDG_CONFIG_HOME"] = self._temp_config
        env["XDG_RUNTIME_DIR"] = self._temp_runtime
        # Strip all WeezTerm/WezTerm env vars to prevent leaks
        for key in list(env.keys()):
            if key.startswith("WEEZTERM_") or key.startswith("WEZTERM_"):
                del env[key]
        return env

    def start(self, extra_args: Optional[list[str]] = None, timeout: float = 30.0) -> float:
        """Launch WeezTerm, wait for window to appear, return startup time in seconds.

        Raises TimeoutError if no window appears within `timeout` seconds.
        """
        if self._process and self._process.poll() is None:
            raise RuntimeError("WeezTerm is already running; call stop() first")

        cmd = [self.binary_path, "--config-file", self._config_file]
        if extra_args:
            cmd.extend(extra_args)

        t0 = time.perf_counter()
        self._process = subprocess.Popen(
            cmd,
            env=self._build_env(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        # Poll for the main window
        deadline = t0 + timeout
        self._hwnd = 0
        while time.perf_counter() < deadline:
            # Check if process died
            if self._process.poll() is not None:
                raise RuntimeError(
                    f"WeezTerm exited with code {self._process.returncode} during startup"
                )
            windows = _find_windows_for_pid(self._process.pid)
            if windows:
                self._hwnd = windows[0]
                break
            time.sleep(0.1)

        if not self._hwnd:
            self._process.kill()
            raise TimeoutError(
                f"WeezTerm did not show a window within {timeout}s"
            )

        # Wait a bit for the window to finish initial rendering
        try:
            proc_handle = kernel32.OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, False, self._process.pid
            )
            if proc_handle:
                user32.WaitForInputIdle(proc_handle, 10000)  # 10s max
                kernel32.CloseHandle(proc_handle)
        except Exception:
            pass

        self._startup_time_s = time.perf_counter() - t0

        # Bring window to foreground so resize/manipulation is visible
        user32.SetForegroundWindow(self._hwnd)

        return self._startup_time_s

    def stop(self, timeout: float = 5.0):
        """Gracefully close the window, then kill if needed."""
        if not self._process:
            return
        if self._process.poll() is not None:
            # Capture any stderr before clearing
            self._capture_stderr()
            self._process = None
            self._hwnd = 0
            return

        # Try to close the window gracefully
        WM_CLOSE = 0x0010
        if self._hwnd:
            user32.PostMessageW(self._hwnd, WM_CLOSE, 0, 0)
            try:
                self._process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=2)
        else:
            self._process.kill()
            self._process.wait(timeout=2)

        self._process = None
        self._hwnd = 0

    def _capture_stderr(self):
        """Capture stderr from the process if available."""
        if self._process and self._process.stderr:
            try:
                self._last_stderr = self._process.stderr.read().decode("utf-8", errors="replace")
            except Exception:
                pass

    @property
    def last_stderr(self) -> str:
        """Last captured stderr output (useful for crash diagnostics)."""
        if self._process and self._process.poll() is not None:
            self._capture_stderr()
        return self._last_stderr

    def cleanup(self):
        """Stop the process and remove temp directories."""
        self.stop()
        shutil.rmtree(self._temp_config, ignore_errors=True)
        shutil.rmtree(self._temp_runtime, ignore_errors=True)

    @property
    def hwnd(self) -> int:
        """Main window handle. 0 if not running."""
        return self._hwnd

    @property
    def pid(self) -> Optional[int]:
        """Process ID, or None if not running."""
        return self._process.pid if self._process else None

    @property
    def is_running(self) -> bool:
        return self._process is not None and self._process.poll() is None

    @property
    def startup_time_s(self) -> float:
        return self._startup_time_s

    @property
    def config_dir(self) -> str:
        """Path to the isolated config directory."""
        return os.path.join(self._temp_config, "weezterm")

    @property
    def window_state_file(self) -> str:
        """Path to the window-state.json in the isolated config dir."""
        return os.path.join(self.config_dir, "window-state.json")

    def start_ssh_mux(
        self,
        domain_name: str,
        remote_address: str = "",
        workspace: Optional[str] = None,
        timeout: float = 60.0,
    ) -> float:
        """Launch WeezTerm connected to a remote host via SSH.

        Uses the `ssh` subcommand (direct SSH, NOT mux multiplexing) to create
        a fully isolated connection. This avoids the --workspace crash bug in
        the `connect` subcommand and completely avoids touching any existing
        mux sessions.

        Args:
            domain_name: Used for logging only.
            remote_address: SSH host to connect to.
            workspace: Not used (SSH sessions are inherently isolated).
            timeout: Seconds to wait for window to appear.

        Returns:
            Startup time in seconds (includes SSH connection time).
        """
        if self._process and self._process.poll() is None:
            raise RuntimeError("WeezTerm is already running; call stop() first")

        # Write minimal config (no SSH domains needed for `ssh` subcommand)
        with open(self._config_file, "w") as f:
            f.write(self.config_lua)

        host = remote_address or domain_name

        # Use `ssh` subcommand — direct SSH session, no mux protocol.
        # This is completely isolated from any existing mux sessions.
        cmd = [
            self.binary_path,
            "--config-file", self._config_file,
            "ssh", host,
        ]

        t0 = time.perf_counter()
        self._process = subprocess.Popen(
            cmd,
            env=self._build_env(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )

        # SSH connection takes a few seconds — poll with patience
        deadline = t0 + timeout
        self._hwnd = 0
        while time.perf_counter() < deadline:
            if self._process.poll() is not None:
                stderr = ""
                if self._process.stderr:
                    stderr = self._process.stderr.read().decode("utf-8", errors="replace")
                raise RuntimeError(
                    f"WeezTerm exited with code {self._process.returncode} "
                    f"during SSH connection to {host}. "
                    f"Stderr: {stderr[-500:]}"
                )
            windows = _find_windows_for_pid(self._process.pid)
            if windows:
                self._hwnd = windows[0]
                break
            time.sleep(0.2)

        if not self._hwnd:
            self._process.kill()
            raise TimeoutError(
                f"WeezTerm did not show a window within {timeout}s "
                f"when connecting via SSH to '{host}'"
            )

        # Wait for the window to become responsive
        try:
            proc_handle = kernel32.OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, False, self._process.pid
            )
            if proc_handle:
                user32.WaitForInputIdle(proc_handle, 30000)  # 30s for SSH
                kernel32.CloseHandle(proc_handle)
        except Exception:
            pass

        self._startup_time_s = time.perf_counter() - t0

        # Bring window to foreground
        user32.SetForegroundWindow(self._hwnd)

        return self._startup_time_s
