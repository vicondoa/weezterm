"""Timing utilities for UX tests.

Provides measurement wrappers for startup time and operation latency.
"""

import statistics
import time
from dataclasses import dataclass, field
from typing import Callable


@dataclass
class TimingResult:
    """Aggregated timing result from multiple samples."""

    samples_ms: list[float] = field(default_factory=list)

    @property
    def count(self) -> int:
        return len(self.samples_ms)

    @property
    def min_ms(self) -> float:
        return min(self.samples_ms) if self.samples_ms else 0.0

    @property
    def max_ms(self) -> float:
        return max(self.samples_ms) if self.samples_ms else 0.0

    @property
    def avg_ms(self) -> float:
        return statistics.mean(self.samples_ms) if self.samples_ms else 0.0

    @property
    def median_ms(self) -> float:
        return statistics.median(self.samples_ms) if self.samples_ms else 0.0

    @property
    def p95_ms(self) -> float:
        if not self.samples_ms:
            return 0.0
        sorted_samples = sorted(self.samples_ms)
        idx = int(len(sorted_samples) * 0.95)
        idx = min(idx, len(sorted_samples) - 1)
        return sorted_samples[idx]

    def summary(self) -> str:
        return (
            f"n={self.count}, min={self.min_ms:.0f}ms, avg={self.avg_ms:.0f}ms, "
            f"median={self.median_ms:.0f}ms, p95={self.p95_ms:.0f}ms, max={self.max_ms:.0f}ms"
        )


def measure_operation(func: Callable, settle_time: float = 0.0) -> float:
    """Time a single operation in milliseconds.

    Args:
        func: The operation to time.
        settle_time: Additional wait after the operation before returning.

    Returns:
        Elapsed time in milliseconds.
    """
    t0 = time.perf_counter()
    func()
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    if settle_time > 0:
        time.sleep(settle_time)
    return elapsed_ms
