"""Screenshot capture and artifact detection for UX tests.

Uses mss for GPU-compatible screen capture and numpy/PIL for analysis.
Detects rendering artifacts like black regions and color bands that
indicate incomplete redraws.
"""

import os
from dataclasses import dataclass
from datetime import datetime
from typing import Optional

import mss
import numpy as np
from PIL import Image

from .window_ops import get_window_rect, WindowRect

# Directory for saving failure screenshots
RESULTS_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), "test-results")


@dataclass
class ArtifactRegion:
    """A detected artifact region in a screenshot."""

    x: int
    y: int
    width: int
    height: int
    area_pixels: int
    area_pct: float  # percentage of total image area
    kind: str  # "black", "band", etc.

    def __repr__(self):
        return (
            f"Artifact({self.kind}: {self.width}x{self.height} at ({self.x},{self.y}), "
            f"{self.area_pct:.1f}%)"
        )


def capture_window(hwnd: int) -> Image.Image:
    """Capture a window's screen region using mss (works with GPU rendering).

    Args:
        hwnd: Window handle to capture.

    Returns:
        PIL Image of the window contents.
    """
    rect = get_window_rect(hwnd)
    # Guard against invalid/zero-sized windows
    if rect.width <= 0 or rect.height <= 0:
        raise RuntimeError(
            f"Cannot capture window: invalid rect {rect} (window may have been destroyed)"
        )
    monitor = {
        "left": rect.x,
        "top": rect.y,
        "width": rect.width,
        "height": rect.height,
    }
    # Create a fresh mss instance each time to avoid _thread._local errors
    # that occur when the window geometry changes between captures
    try:
        with mss.mss() as sct:
            screenshot = sct.grab(monitor)
            img = Image.frombytes("RGB", screenshot.size, screenshot.bgra, "raw", "BGRX")
    except (AttributeError, OSError) as e:
        # Fallback: use PIL's ImageGrab if mss fails
        from PIL import ImageGrab
        img = ImageGrab.grab(bbox=(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height))
    return img


def detect_rendering_artifacts(
    image: Image.Image, threshold: int = 15
) -> list[ArtifactRegion]:
    """Detect rendering artifacts in a terminal window screenshot.

    A terminal window is EXPECTED to be mostly dark/black (the terminal
    background). Instead of naively looking for black pixels, we detect
    actual artifacts by checking structural elements:

    1. **Tab bar discontinuity**: The tab bar (~top 40px) should span the
       full width. If it stops partway, the right/bottom portion is undrawn.
    2. **Vertical/horizontal split**: One half has rendered content (tab bar,
       prompt, status bar) and the other half is uniformly pure black with
       no UI elements at all.

    Args:
        image: PIL Image to analyze.
        threshold: Maximum RGB channel value to consider "pure black" (0-255).

    Returns:
        List of detected artifact regions.
    """
    arr = np.array(image)
    if arr.size == 0:
        return []

    h, w = arr.shape[:2]
    if h < 60 or w < 60:
        return []  # too small to analyze

    artifacts = []

    # Strategy 1: Tab bar discontinuity
    # The tab bar is in the top ~40px. It has non-black content (tab labels,
    # buttons, background color). Sample a horizontal band at the tab bar.
    tab_bar_region = arr[10:45, :, :3]  # rows 10-45, all columns
    # For each column, check if the tab bar band has any non-black pixels
    col_has_content = np.any(np.any(tab_bar_region > threshold, axis=2), axis=0)

    # Find the rightmost column with tab bar content
    content_cols = np.where(col_has_content)[0]
    if len(content_cols) > 0:
        rightmost_content = int(content_cols[-1])
        # If tab bar content ends well before the right edge, that's an artifact
        gap = w - rightmost_content
        if gap > w * 0.15:  # >15% of width is undrawn after tab bar ends
            artifact_width = gap
            artifacts.append(ArtifactRegion(
                x=rightmost_content,
                y=0,
                width=artifact_width,
                height=h,
                area_pixels=artifact_width * h,
                area_pct=(artifact_width * h) / (w * h) * 100,
                kind="tabbar-discontinuity-right",
            ))

    # Strategy 2: Bottom status bar discontinuity
    # Check if the bottom ~25px has content spanning the full width
    if h > 50:
        bottom_region = arr[h - 25:h, :, :3]
        bottom_has_content = np.any(np.any(bottom_region > threshold, axis=2), axis=0)
        bottom_content_cols = np.where(bottom_has_content)[0]
        if len(bottom_content_cols) > 0:
            rightmost_bottom = int(bottom_content_cols[-1])
            bottom_gap = w - rightmost_bottom
            if bottom_gap > w * 0.15:
                artifacts.append(ArtifactRegion(
                    x=rightmost_bottom,
                    y=0,
                    width=bottom_gap,
                    height=h,
                    area_pixels=bottom_gap * h,
                    area_pct=(bottom_gap * h) / (w * h) * 100,
                    kind="statusbar-discontinuity-right",
                ))

    # Strategy 3: Horizontal split (content on top, pure black on bottom)
    # Check row-by-row: if upper rows have content but lower rows are 100% black
    row_has_content = np.any(np.any(arr[:, :, :3] > threshold, axis=2), axis=1)
    content_rows = np.where(row_has_content)[0]
    if len(content_rows) > 0:
        lowest_content = int(content_rows[-1])
        bottom_gap = h - lowest_content
        if bottom_gap > h * 0.15:
            artifacts.append(ArtifactRegion(
                x=0,
                y=lowest_content,
                width=w,
                height=bottom_gap,
                area_pixels=w * bottom_gap,
                area_pct=(w * bottom_gap) / (w * h) * 100,
                kind="split-bottom",
            ))

    return artifacts


def detect_black_regions(
    image: Image.Image, threshold: int = 15, min_area_pct: float = 2.0
) -> list[ArtifactRegion]:
    """Detect large contiguous near-black regions in a screenshot.

    NOTE: A terminal window is normally mostly dark. This function is kept
    for raw analysis but prefer detect_rendering_artifacts() for artifact
    detection — it understands terminal window structure.

    Args:
        image: PIL Image to analyze.
        threshold: Maximum RGB channel value to consider "black" (0-255).
        min_area_pct: Minimum region size as percentage of total image to report.

    Returns:
        List of detected artifact regions.
    """
    arr = np.array(image)
    if arr.size == 0:
        return []

    total_pixels = arr.shape[0] * arr.shape[1]
    is_black = np.all(arr[:, :, :3] <= threshold, axis=2)
    black_pixel_count = int(np.sum(is_black))
    black_pct = (black_pixel_count / total_pixels) * 100.0

    if black_pct < min_area_pct:
        return []

    rows = np.any(is_black, axis=1)
    cols = np.any(is_black, axis=0)

    if not rows.any():
        return []

    rmin, rmax = np.where(rows)[0][[0, -1]]
    cmin, cmax = np.where(cols)[0][[0, -1]]

    return [ArtifactRegion(
        x=int(cmin), y=int(rmin),
        width=int(cmax - cmin + 1), height=int(rmax - rmin + 1),
        area_pixels=black_pixel_count, area_pct=black_pct, kind="black",
    )]


def image_black_percentage(image: Image.Image, threshold: int = 15) -> float:
    """Calculate what percentage of the image is near-black.

    Args:
        image: PIL Image to analyze.
        threshold: Maximum RGB channel value to consider "black".

    Returns:
        Percentage (0-100) of near-black pixels.
    """
    arr = np.array(image)
    if arr.size == 0:
        return 0.0
    is_black = np.all(arr[:, :, :3] <= threshold, axis=2)
    return float(np.mean(is_black) * 100.0)


def save_screenshot(
    image: Image.Image,
    test_name: str,
    suffix: str = "",
    directory: Optional[str] = None,
) -> str:
    """Save a screenshot for debugging/CI artifact collection.

    Args:
        image: PIL Image to save.
        test_name: Name of the test (used in filename).
        suffix: Optional suffix for the filename.
        directory: Override output directory.

    Returns:
        Path to saved file.
    """
    out_dir = directory or RESULTS_DIR
    os.makedirs(out_dir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    suffix_part = f"_{suffix}" if suffix else ""
    filename = f"{test_name}{suffix_part}_{ts}.png"
    path = os.path.join(out_dir, filename)
    image.save(path)
    return path
