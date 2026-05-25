#!/usr/bin/env python3
"""TUI resize test display — draws a bordered screen with debug markers.

Run this on a remote host via SSH to test terminal resize behavior.
It draws:
- A border around the entire terminal
- Corner markers showing the exact terminal dimensions
- A grid pattern to detect content stretching/misalignment
- A center crosshair
- A resize counter that updates on SIGWINCH

Usage:
    python3 tui_resize_test.py          # runs until Ctrl+C
    python3 tui_resize_test.py --once   # draw once and exit (for screenshots)
"""

import curses
import signal
import sys
import time

resize_count = 0


def draw_screen(stdscr):
    """Draw the test pattern on the terminal."""
    global resize_count

    stdscr.clear()
    height, width = stdscr.getmaxyx()

    if height < 3 or width < 10:
        stdscr.addstr(0, 0, "TOO SMALL")
        stdscr.refresh()
        return

    # Use safe dimensions (curses can't write to bottom-right corner)
    max_y = height - 1
    max_x = width - 1

    # Draw border
    for x in range(min(width, max_x + 1)):
        try:
            stdscr.addch(0, x, curses.ACS_HLINE)
            stdscr.addch(max_y, x, curses.ACS_HLINE)
        except curses.error:
            pass
    for y in range(min(height, max_y + 1)):
        try:
            stdscr.addch(y, 0, curses.ACS_VLINE)
            stdscr.addch(y, max_x, curses.ACS_VLINE)
        except curses.error:
            pass

    # Corners
    try:
        stdscr.addch(0, 0, curses.ACS_ULCORNER)
        stdscr.addch(0, max_x, curses.ACS_URCORNER)
        stdscr.addch(max_y, 0, curses.ACS_LLCORNER)
    except curses.error:
        pass
    # Bottom-right corner: can't write to last position in curses
    try:
        stdscr.addch(max_y, max_x - 1, curses.ACS_HLINE)
    except curses.error:
        pass

    # Dimension labels at corners
    dim_str = f"{width}x{height}"
    try:
        stdscr.addstr(0, 2, f" {dim_str} ")
        stdscr.addstr(max_y, 2, f" {dim_str} ")
        stdscr.addstr(0, max_x - len(dim_str) - 3, f" {dim_str} ")
    except curses.error:
        pass

    # Grid lines every 10 columns and rows
    for x in range(10, max_x, 10):
        try:
            stdscr.addch(0, x, curses.ACS_TTEE)
            stdscr.addch(max_y, x, curses.ACS_BTEE)
        except curses.error:
            pass
        for y in range(1, max_y):
            try:
                stdscr.addch(y, x, curses.ACS_VLINE)
            except curses.error:
                pass

    for y in range(5, max_y, 5):
        try:
            stdscr.addch(y, 0, curses.ACS_LTEE)
            stdscr.addch(y, max_x, curses.ACS_RTEE)
        except curses.error:
            pass
        for x in range(1, max_x):
            if x % 10 == 0:
                try:
                    stdscr.addch(y, x, curses.ACS_PLUS)
                except curses.error:
                    pass
            else:
                try:
                    stdscr.addch(y, x, curses.ACS_HLINE)
                except curses.error:
                    pass

    # Row/column number labels
    for y in range(5, max_y, 5):
        label = f" r{y} "
        try:
            stdscr.addstr(y, 1, label)
        except curses.error:
            pass
    for x in range(10, max_x, 10):
        label = f"c{x}"
        try:
            stdscr.addstr(1, x - len(label) // 2, label)
        except curses.error:
            pass

    # Center crosshair
    cx, cy = width // 2, height // 2
    if cy > 1 and cy < max_y and cx > 1 and cx < max_x:
        for x in range(max(1, cx - 5), min(max_x, cx + 6)):
            try:
                stdscr.addch(cy, x, curses.ACS_HLINE)
            except curses.error:
                pass
        for y in range(max(1, cy - 3), min(max_y, cy + 4)):
            try:
                stdscr.addch(y, cx, curses.ACS_VLINE)
            except curses.error:
                pass
        try:
            stdscr.addch(cy, cx, curses.ACS_PLUS)
        except curses.error:
            pass

    # Status line
    status = f" Resize #{resize_count} | {width}x{height} | Ctrl+C to quit "
    try:
        stdscr.addstr(height // 2 - 2, max(1, cx - len(status) // 2), status,
                       curses.A_REVERSE)
    except curses.error:
        pass

    # Fill each quadrant with a marker character to detect stretching
    markers = [
        (height // 4, width // 4, "UL"),
        (height // 4, 3 * width // 4, "UR"),
        (3 * height // 4, width // 4, "LL"),
        (3 * height // 4, 3 * width // 4, "LR"),
    ]
    for my, mx, label in markers:
        if 1 < my < max_y and 1 < mx < max_x - 2:
            try:
                stdscr.addstr(my, mx, label, curses.A_BOLD)
            except curses.error:
                pass

    stdscr.refresh()


def main(stdscr):
    global resize_count

    # --- weezterm remote features ---
    # Diagnostic log: write SIGWINCH events and observed sizes to a file
    # so we can verify whether the remote side actually receives resize.
    import os as _os
    _diag_log = open("/tmp/tui_resize_diag.log", "a", buffering=1)
    _diag_log.write(f"\n=== TUI started pid={_os.getpid()} initial_size={stdscr.getmaxyx()} ===\n")
    # --- end weezterm remote features ---

    curses.curs_set(0)  # hide cursor
    stdscr.timeout(100)  # 100ms timeout for getch

    # --- weezterm remote features ---
    # NOTE: Do NOT install a Python signal.signal(SIGWINCH, ...) handler.
    # ncurses installs its own SIGWINCH handler at initscr() time which
    # updates LINES/COLS and posts KEY_RESIZE. A Python-level handler
    # overrides that, breaking curses' internal size tracking.
    # --- end weezterm remote features ---

    once = "--once" in sys.argv

    draw_screen(stdscr)

    if once:
        time.sleep(0.5)
        return

    last_size = None
    # --- weezterm remote features ---
    last_loglog = time.time()
    # --- end weezterm remote features ---
    while True:
        try:
            key = stdscr.getch()
            if key == ord("q") or key == 3:  # q or Ctrl+C
                break
            if key == curses.KEY_RESIZE or key == -1:
                # --- weezterm remote features ---
                if key == curses.KEY_RESIZE:
                    resize_count += 1
                    try:
                        _diag_log.write(f"KEY_RESIZE #{resize_count} size={stdscr.getmaxyx()}\n")
                    except Exception:
                        pass
                # --- end weezterm remote features ---
                current_size = stdscr.getmaxyx()
                if current_size != last_size:
                    # --- weezterm remote features ---
                    _diag_log.write(f"main_loop size_changed {last_size} -> {current_size}\n")
                    # --- end weezterm remote features ---
                    last_size = current_size
                    draw_screen(stdscr)
                # --- weezterm remote features ---
                else:
                    now = time.time()
                    if now - last_loglog > 1.0:
                        _diag_log.write(f"main_loop heartbeat size={current_size} count={resize_count}\n")
                        last_loglog = now
                # --- end weezterm remote features ---
        except KeyboardInterrupt:
            break


if __name__ == "__main__":
    curses.wrapper(main)
