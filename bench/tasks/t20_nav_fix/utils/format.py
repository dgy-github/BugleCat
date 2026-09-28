"""Shared formatting helpers for the order CLI."""


def format_duration(seconds: int) -> str:
    if seconds < 0:
        raise ValueError("seconds must be non-negative")
    minutes = seconds // 60
    rest = seconds % 60
    return f"{minutes}m{rest}s"
