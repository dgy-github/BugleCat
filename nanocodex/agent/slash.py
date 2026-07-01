"""Parse REPL slash commands into ``(command, argument)``.

Pure string logic so the dispatcher is unit-tested without a console: this
module only recognizes and splits a slash line; the REPL maps the parsed command
to an action (show status, switch model, compact, …).
"""

from __future__ import annotations

import re

# command -> one-line help, shown by /help. Insertion order is display order.
SLASH_HELP: "dict[str, str]" = {
    "/help": "Show this help.",
    "/status": "Show model, sandbox, approval, workspace, and token usage.",
    "/model": "Show the current model, or switch it: /model <name>.",
    "/approvals": "Show the approval policy, or set it: /approvals <untrusted|on-failure|on-request|never>.",
    "/diff": "Show the working-tree git diff.",
    "/plan": "Show the current step plan.",
    "/loop": "Repeat a prompt on an interval: /loop [5m] <prompt> (Ctrl+C stops).",
    "/compact": "Fold the conversation now to the token budget.",
    "/clear": "Start a fresh conversation (keep settings).",
    "/exit": "Quit the REPL (also /quit).",
}

# Default /loop interval when none is given (matches Claude Code's /loop: 10m).
DEFAULT_LOOP_INTERVAL_S = 600

_DURATION = re.compile(r"^(\d+(?:\.\d+)?)\s*([smh]?)$", re.IGNORECASE)


def parse_duration(text: str) -> "int | None":
    """Parse a human duration into whole seconds, or None if it isn't one.

    Accepts ``30s`` / ``5m`` / ``1h`` and a bare number (seconds). Returns None
    for anything else (so the caller can treat it as part of the prompt instead
    of an interval). Zero/negative durations are rejected.
    """
    m = _DURATION.match((text or "").strip())
    if not m:
        return None
    factor = {"": 1, "s": 1, "m": 60, "h": 3600}[m.group(2).lower()]
    secs = int(float(m.group(1)) * factor)
    return secs if secs > 0 else None


def split_loop_arg(arg: str, *, default_s: int = DEFAULT_LOOP_INTERVAL_S
                   ) -> "tuple[int, str]":
    """Split a /loop argument into ``(interval_seconds, prompt)``.

    A leading token that parses as a duration becomes the interval and the rest
    is the prompt (``5m run the tests``); otherwise the whole argument is the
    prompt at *default_s* (``run the tests`` -> 10m). The prompt may be empty,
    which the caller should reject.
    """
    parts = (arg or "").split(None, 1)
    if len(parts) == 2:
        dur = parse_duration(parts[0])
        if dur is not None:
            return dur, parts[1].strip()
    return default_s, (arg or "").strip()


def parse_slash(text: str) -> "tuple[str | None, str]":
    """Return ``(command, arg)`` for a slash line, or ``(None, "")`` if not one.

    A command is a line whose first non-space character is ``/``. The command
    token is lower-cased; the remainder (stripped) is the argument. ``/quit``
    normalizes to ``/exit``.
    """
    s = (text or "").strip()
    if not s.startswith("/"):
        return None, ""
    parts = s.split(None, 1)
    cmd = parts[0].lower()
    arg = parts[1].strip() if len(parts) > 1 else ""
    if cmd == "/quit":
        cmd = "/exit"
    return cmd, arg
