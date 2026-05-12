"""Win32 API wrappers for window manipulation.

Provides functions to resize, move, maximize, minimize, and query
window state using ctypes calls to user32.dll.
"""

import ctypes
import ctypes.wintypes
import time
from dataclasses import dataclass

user32 = ctypes.windll.user32

# ShowWindow commands
SW_HIDE = 0
SW_NORMAL = 1
SW_MINIMIZE = 6
SW_MAXIMIZE = 3
SW_RESTORE = 9

# Window messages
WM_SIZE = 0x0005


class WINDOWPLACEMENT(ctypes.Structure):
    _fields_ = [
        ("length", ctypes.wintypes.UINT),
        ("flags", ctypes.wintypes.UINT),
        ("showCmd", ctypes.wintypes.UINT),
        ("ptMinPosition", ctypes.wintypes.POINT),
        ("ptMaxPosition", ctypes.wintypes.POINT),
        ("rcNormalPosition", ctypes.wintypes.RECT),
    ]


@dataclass
class WindowRect:
    """Window rectangle with position and size."""

    x: int
    y: int
    width: int
    height: int

    def __repr__(self):
        return f"WindowRect(x={self.x}, y={self.y}, w={self.width}, h={self.height})"


def get_window_rect(hwnd: int) -> WindowRect:
    """Get the window's outer rectangle (including decorations)."""
    rect = ctypes.wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return WindowRect(
        x=rect.left,
        y=rect.top,
        width=rect.right - rect.left,
        height=rect.bottom - rect.top,
    )


def get_client_rect(hwnd: int) -> WindowRect:
    """Get the window's client area rectangle (excluding decorations)."""
    rect = ctypes.wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    # Client rect is relative to client area, convert to screen coords
    pt = ctypes.wintypes.POINT(rect.left, rect.top)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return WindowRect(
        x=pt.x,
        y=pt.y,
        width=rect.right - rect.left,
        height=rect.bottom - rect.top,
    )


def set_window_rect(hwnd: int, x: int, y: int, width: int, height: int):
    """Move and resize a window."""
    user32.MoveWindow(hwnd, x, y, width, height, True)


def maximize(hwnd: int):
    """Maximize the window."""
    user32.ShowWindow(hwnd, SW_MAXIMIZE)


def restore(hwnd: int):
    """Restore (unmaximize) the window."""
    user32.ShowWindow(hwnd, SW_RESTORE)


def minimize(hwnd: int):
    """Minimize the window."""
    user32.ShowWindow(hwnd, SW_MINIMIZE)


def is_maximized(hwnd: int) -> bool:
    """Check if the window is maximized."""
    return bool(user32.IsZoomed(hwnd))


def is_minimized(hwnd: int) -> bool:
    """Check if the window is minimized (iconic)."""
    return bool(user32.IsIconic(hwnd))


def is_visible(hwnd: int) -> bool:
    """Check if the window is visible."""
    return bool(user32.IsWindowVisible(hwnd))


def get_window_placement(hwnd: int) -> WINDOWPLACEMENT:
    """Get the full WINDOWPLACEMENT struct (includes normal position)."""
    wp = WINDOWPLACEMENT()
    wp.length = ctypes.sizeof(WINDOWPLACEMENT)
    user32.GetWindowPlacement(hwnd, ctypes.byref(wp))
    return wp


def get_normal_rect(hwnd: int) -> WindowRect:
    """Get the window's 'normal' (restored) rectangle from WINDOWPLACEMENT.

    This is the size/position the window had before being maximized,
    and is what it should return to when restored.
    """
    wp = get_window_placement(hwnd)
    r = wp.rcNormalPosition
    return WindowRect(
        x=r.left,
        y=r.top,
        width=r.right - r.left,
        height=r.bottom - r.top,
    )


def set_foreground(hwnd: int):
    """Bring the window to the foreground."""
    user32.SetForegroundWindow(hwnd)


def wait_for_idle(hwnd: int, timeout_ms: int = 5000):
    """Wait for the window's thread to become idle."""
    tid = user32.GetWindowThreadProcessId(hwnd, None)
    if tid:
        # Attach to thread and wait
        time.sleep(0.1)  # Simple fallback


def settle(delay: float = 0.5):
    """Wait for UI to settle after an operation."""
    time.sleep(delay)


# --- weezterm remote features ---
def get_work_area() -> WindowRect:
    """Get the work area (screen minus taskbar) of the primary monitor."""
    rect = ctypes.wintypes.RECT()
    SPI_GETWORKAREA = 0x0030
    ctypes.windll.user32.SystemParametersInfoW(
        SPI_GETWORKAREA, 0, ctypes.byref(rect), 0
    )
    return WindowRect(
        x=rect.left,
        y=rect.top,
        width=rect.right - rect.left,
        height=rect.bottom - rect.top,
    )


def snap_left(hwnd: int):
    """Simulate Windows Snap Left (Win+Left arrow).

    Uses keybd_event to send Win+Left inside the session.
    This works over RDP because it runs within the remote session.
    """
    set_foreground(hwnd)
    time.sleep(0.2)
    _send_snap_key(0x25)  # VK_LEFT


def snap_right(hwnd: int):
    """Simulate Windows Snap Right (Win+Right arrow)."""
    set_foreground(hwnd)
    time.sleep(0.2)
    _send_snap_key(0x27)  # VK_RIGHT


def _send_snap_key(arrow_vk: int):
    """Send Win+Arrow via keybd_event for window snapping."""
    VK_LWIN = 0x5B
    KEYEVENTF_KEYUP = 0x0002
    user32.keybd_event(VK_LWIN, 0, 0, 0)           # Win down
    time.sleep(0.05)
    user32.keybd_event(arrow_vk, 0, 0, 0)           # Arrow down
    time.sleep(0.05)
    user32.keybd_event(arrow_vk, 0, KEYEVENTF_KEYUP, 0)  # Arrow up
    time.sleep(0.05)
    user32.keybd_event(VK_LWIN, 0, KEYEVENTF_KEYUP, 0)   # Win up


def _send_key_combo(modifier_vk: int, key_vk: int):
    """Send a two-key combination via SendInput (e.g. Win+Left)."""

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [
            ("wVk", ctypes.wintypes.WORD),
            ("wScan", ctypes.wintypes.WORD),
            ("dwFlags", ctypes.wintypes.DWORD),
            ("time", ctypes.wintypes.DWORD),
            ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
        ]

    class INPUT(ctypes.Structure):
        class _INPUT_UNION(ctypes.Union):
            _fields_ = [("ki", KEYBDINPUT)]
        _fields_ = [
            ("type", ctypes.wintypes.DWORD),
            ("union", _INPUT_UNION),
        ]

    INPUT_KEYBOARD = 1
    KEYEVENTF_KEYUP = 0x0002

    def make_input(vk, flags=0):
        inp = INPUT()
        inp.type = INPUT_KEYBOARD
        inp.union.ki.wVk = vk
        inp.union.ki.dwFlags = flags
        return inp

    inputs = (INPUT * 4)(
        make_input(modifier_vk),          # modifier down
        make_input(key_vk),               # key down
        make_input(key_vk, KEYEVENTF_KEYUP),    # key up
        make_input(modifier_vk, KEYEVENTF_KEYUP),  # modifier up
    )
    user32.SendInput(4, ctypes.byref(inputs), ctypes.sizeof(INPUT))
# --- end weezterm remote features ---
