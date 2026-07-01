"""Lightweight, dimension-filtered tracing gated on the NCX_TRACE env var.

Mirrors the Rust port's convention (NCX_TRACE non-empty = on) but adds
*dimensions* so a live run can opt into just one subsystem's chatter:

    NCX_TRACE=1            # everything (also: all, *, true, on)
    NCX_TRACE=verifier     # only the "verifier" dimension
    NCX_TRACE=verifier,orchestrator   # a comma-separated allow-list

Trace lines go to stderr so they never pollute a tool result or the model's
stdout-parsed output. This is a debug aid, not structured logging.
"""

from __future__ import annotations

import os
import sys

_ON_VALUES = {"1", "all", "*", "true", "on", "yes"}


def trace_enabled(dimension: str) -> bool:
    """True if NCX_TRACE is on globally or names *dimension* in its allow-list."""
    val = os.environ.get("NCX_TRACE", "").strip()
    if not val:
        return False
    if val.lower() in _ON_VALUES:
        return True
    return dimension in {c.strip() for c in val.split(",") if c.strip()}


def trace(dimension: str, message: str) -> None:
    """Emit a ``[trace:<dimension>] message`` line to stderr when enabled."""
    if trace_enabled(dimension):
        print(f"[trace:{dimension}] {message}", file=sys.stderr, flush=True)
