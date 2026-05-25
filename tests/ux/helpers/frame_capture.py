# --- weezterm remote features ---
"""High-rate continuous window screenshot capture for diagnosing
transient rendering artifacts during resize.

Spawns a background thread that grabs the window every `interval_s`
seconds (default ~16ms = 60fps cap) and saves each frame to a directory
with sequential numbering, a timestamp, and the live window rect.

The capture thread also emits an indexed manifest (`manifest.csv`) so
tests can correlate frame numbers with the timeline of operations the
main thread performed.

Usage (typical):

    from helpers.frame_capture import FrameCapture
    cap = FrameCapture(hwnd, "tests/ux/test-results/<test-name>")
    cap.start()
    cap.note("before resize")
    set_window_rect(hwnd, 100, 100, 1200, 900)
    time.sleep(2.0)
    cap.note("after resize")
    cap.stop()

Each captured frame is named `frame_<index>_<elapsed_ms>ms_<WxH>.png`.
The manifest also records every `note()` call so artifacts (e.g. a
cluster of stretched frames) can be tied to the action that triggered
them. No assertions are made — these tests are purely diagnostic.
"""

import csv
import os
import threading
import time
from typing import Optional

from .screenshot import capture_window
from .window_ops import get_window_rect


class FrameCapture:
    """Continuously capture window frames in a background thread.

    Frames are saved to ``out_dir`` with monotonically increasing
    indices. The capture loop is best-effort — if a frame grab fails
    (window briefly invalid during resize) it logs a row in the manifest
    with `error=...` and continues.

    The thread runs until ``stop()`` is called or until ``max_frames``
    is reached (whichever comes first).
    """

    def __init__(
        self,
        hwnd: int,
        out_dir: str,
        interval_s: float = 0.016,
        max_frames: int = 4000,
    ):
        self.hwnd = hwnd
        self.out_dir = out_dir
        self.interval_s = interval_s
        self.max_frames = max_frames

        self._thread: Optional[threading.Thread] = None
        self._stop_event = threading.Event()
        self._notes_lock = threading.Lock()
        self._notes: list[tuple[float, str]] = []
        self._frames_captured = 0
        self._t_start = 0.0
        self._manifest_rows: list[dict] = []
        self._manifest_lock = threading.Lock()

        os.makedirs(self.out_dir, exist_ok=True)

    @property
    def frames_captured(self) -> int:
        return self._frames_captured

    @property
    def manifest_path(self) -> str:
        return os.path.join(self.out_dir, "manifest.csv")

    def start(self):
        """Start the capture thread. Returns immediately."""
        if self._thread is not None:
            raise RuntimeError("FrameCapture already started")
        self._stop_event.clear()
        self._t_start = time.perf_counter()
        self._thread = threading.Thread(
            target=self._capture_loop,
            name="FrameCapture",
            daemon=True,
        )
        self._thread.start()

    def note(self, message: str):
        """Record a timestamped note (e.g. 'starting resize') in the
        manifest so post-hoc analysis can correlate frame indices with
        the operations the main thread performed."""
        elapsed_ms = (time.perf_counter() - self._t_start) * 1000.0
        with self._notes_lock:
            self._notes.append((elapsed_ms, message))

    def stop(self, timeout: float = 5.0) -> str:
        """Stop the capture thread, write the manifest, and return the
        manifest path."""
        if self._thread is None:
            return self.manifest_path
        self._stop_event.set()
        self._thread.join(timeout=timeout)
        self._thread = None
        self._write_manifest()
        return self.manifest_path

    def _capture_loop(self):
        next_t = time.perf_counter()
        idx = 0
        while not self._stop_event.is_set() and idx < self.max_frames:
            now = time.perf_counter()
            elapsed_ms = (now - self._t_start) * 1000.0

            # Snapshot any pending notes since the last frame so they
            # are written into the manifest in chronological order.
            pending_notes: list[tuple[float, str]] = []
            with self._notes_lock:
                pending_notes, self._notes = self._notes, []
            for note_ms, note_msg in pending_notes:
                with self._manifest_lock:
                    self._manifest_rows.append({
                        "index": "",
                        "elapsed_ms": f"{note_ms:.1f}",
                        "frame_w": "",
                        "frame_h": "",
                        "win_x": "",
                        "win_y": "",
                        "win_w": "",
                        "win_h": "",
                        "filename": "",
                        "kind": "note",
                        "detail": note_msg,
                    })

            try:
                rect = get_window_rect(self.hwnd)
                img = capture_window(self.hwnd)
                w, h = img.size
                fname = f"frame_{idx:04d}_{int(elapsed_ms):07d}ms_{w}x{h}.png"
                fpath = os.path.join(self.out_dir, fname)
                img.save(fpath)
                with self._manifest_lock:
                    self._manifest_rows.append({
                        "index": str(idx),
                        "elapsed_ms": f"{elapsed_ms:.1f}",
                        "frame_w": str(w),
                        "frame_h": str(h),
                        "win_x": str(rect.x),
                        "win_y": str(rect.y),
                        "win_w": str(rect.width),
                        "win_h": str(rect.height),
                        "filename": fname,
                        "kind": "frame",
                        "detail": "",
                    })
                idx += 1
                self._frames_captured = idx
            except Exception as e:
                with self._manifest_lock:
                    self._manifest_rows.append({
                        "index": str(idx),
                        "elapsed_ms": f"{elapsed_ms:.1f}",
                        "frame_w": "",
                        "frame_h": "",
                        "win_x": "",
                        "win_y": "",
                        "win_w": "",
                        "win_h": "",
                        "filename": "",
                        "kind": "error",
                        "detail": str(e),
                    })

            next_t += self.interval_s
            sleep_for = next_t - time.perf_counter()
            if sleep_for > 0:
                # Use a short event wait so stop() is responsive without
                # busy-spinning.
                if self._stop_event.wait(timeout=sleep_for):
                    break
            else:
                # We've fallen behind: reset the cadence to "now" so we
                # don't burn CPU trying to catch up.
                next_t = time.perf_counter()

        # Drain any final notes that arrived between the last frame and
        # stop().
        with self._notes_lock:
            pending_notes, self._notes = self._notes, []
        for note_ms, note_msg in pending_notes:
            with self._manifest_lock:
                self._manifest_rows.append({
                    "index": "",
                    "elapsed_ms": f"{note_ms:.1f}",
                    "frame_w": "",
                    "frame_h": "",
                    "win_x": "",
                    "win_y": "",
                    "win_w": "",
                    "win_h": "",
                    "filename": "",
                    "kind": "note",
                    "detail": note_msg,
                })

    def _write_manifest(self):
        """Write the manifest CSV. Sorted by elapsed_ms ascending."""
        with self._manifest_lock:
            rows = list(self._manifest_rows)
        rows.sort(key=lambda r: float(r["elapsed_ms"]) if r["elapsed_ms"] else 0.0)
        cols = [
            "index",
            "elapsed_ms",
            "frame_w",
            "frame_h",
            "win_x",
            "win_y",
            "win_w",
            "win_h",
            "filename",
            "kind",
            "detail",
        ]
        with open(self.manifest_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=cols)
            writer.writeheader()
            for row in rows:
                writer.writerow(row)


def summarize_frames(out_dir: str) -> str:
    """Read a frame-capture directory and return a one-paragraph
    summary suitable for printing at the end of a test."""
    manifest = os.path.join(out_dir, "manifest.csv")
    if not os.path.isfile(manifest):
        return f"(no manifest at {manifest})"
    frames = 0
    notes = 0
    errors = 0
    sizes = set()
    with open(manifest, "r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            kind = row.get("kind", "")
            if kind == "frame":
                frames += 1
                if row["frame_w"] and row["frame_h"]:
                    sizes.add((row["frame_w"], row["frame_h"]))
            elif kind == "note":
                notes += 1
            elif kind == "error":
                errors += 1
    distinct_sizes = ",".join(f"{w}x{h}" for w, h in sorted(sizes))
    return (
        f"FrameCapture summary: dir={out_dir} frames={frames} "
        f"notes={notes} errors={errors} distinct_sizes=[{distinct_sizes}]"
    )
# --- end weezterm remote features ---
