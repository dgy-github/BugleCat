"""Scheduled tasks: run a saved prompt automatically at a future time.

A scheduled task is just "a prompt + when to run it", persisted as plain JSON
the user controls (mirrors the rest of nanocodex — no hidden state, no heavy
deps). Three recurrence kinds cover the common cases:

* ``once``     — run a single time at ``run_at`` (an ISO timestamp).
* ``interval`` — run every ``every_seconds`` seconds, starting at ``run_at``.
* ``daily``    — run every day at ``at_hour:at_minute`` (local time).

Design split (same spirit as memory_store / window finder): the STORE and the
DUE-CALCULATION are pure functions over data + an injected "now", so they unit
test offline with zero clocks or threads. The actual "wait and run" loop lives
in the runner (schedule_runner) and just drives these.
"""

from __future__ import annotations

import json
import uuid
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

DEFAULT_SCHEDULE_PATH = Path.home() / ".nanocodex" / "schedule.json"

VALID_KINDS = ("once", "interval", "daily")
_ISO = "%Y-%m-%dT%H:%M:%S"

# A recurring task that fails (times out / errors) this many times in a row with
# no success in between is auto-disabled, so a misconfigured task (e.g. one that
# needs desktop tools but was created without allow_desktop) can't spin forever
# burning tokens every poll. A single success resets the counter.
MAX_CONSECUTIVE_FAILURES = 5


def _now() -> datetime:
    return datetime.now()


def _parse_iso(value: str) -> datetime | None:
    if not value:
        return None
    try:
        # Tolerate a trailing microseconds/space; take the date+time head.
        return datetime.strptime(value[:19], _ISO)
    except (ValueError, TypeError):
        return None


@dataclass
class ScheduledTask:
    """One saved task. ``next_run`` is the ISO time it should fire next."""

    id: str
    prompt: str
    kind: str = "once"                 # once | interval | daily
    next_run: str = ""                 # ISO timestamp of the next firing
    every_seconds: int = 0             # for kind == "interval"
    at_hour: int = 9                   # for kind == "daily"
    at_minute: int = 0                 # for kind == "daily"
    enabled: bool = True
    last_run: str = ""                 # ISO timestamp of the last firing
    runs: int = 0                      # how many times it has fired
    consecutive_failures: int = 0      # failures in a row (reset on success)
    # SECURITY: when True, this task's unattended runs may drive DESKTOP (MCP)
    # actions — clicking/typing into real apps with no human watching. Default
    # False keeps the safe behavior (the scheduler's auto-deny approver refuses
    # every escalated/out-of-sandbox action). Only ever set this for a task whose
    # prompt you fully trust; it never widens shell / out-of-sandbox file access.
    allow_desktop: bool = False

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def compute_next_run(task: ScheduledTask, after: datetime) -> str | None:
    """Return the next ISO firing time strictly after *after*, or None.

    ``once`` has no next run after it fires (returns None). ``interval`` advances
    by whole periods until it is past *after*. ``daily`` lands on the next
    at_hour:at_minute that is after *after*.
    """
    if task.kind == "once":
        return None
    if task.kind == "interval":
        step = max(1, int(task.every_seconds))
        base = _parse_iso(task.next_run) or after
        nxt = base
        # Advance in whole steps until strictly after *after*.
        while nxt <= after:
            nxt = nxt + timedelta(seconds=step)
        return nxt.strftime(_ISO)
    if task.kind == "daily":
        candidate = after.replace(hour=int(task.at_hour), minute=int(task.at_minute),
                                  second=0, microsecond=0)
        if candidate <= after:
            candidate = candidate + timedelta(days=1)
        return candidate.strftime(_ISO)
    return None


def _initial_next_run(task: ScheduledTask, now: datetime) -> str:
    """First firing time when a task is created."""
    if task.kind == "daily":
        return compute_next_run(task, now) or ""
    if task.kind == "interval":
        # If a start (next_run) was given and is in the future, keep it;
        # otherwise start one period from now.
        start = _parse_iso(task.next_run)
        if start and start > now:
            return start.strftime(_ISO)
        step = max(1, int(task.every_seconds))
        return (now + timedelta(seconds=step)).strftime(_ISO)
    # once: keep the provided next_run, else run (almost) immediately.
    start = _parse_iso(task.next_run)
    return (start or now).strftime(_ISO)


class ScheduleStore:
    """Load/save scheduled tasks as plain JSON, and compute what's due."""

    def __init__(self, path: Path | None = None) -> None:
        self.path = Path(path) if path is not None else DEFAULT_SCHEDULE_PATH
        self.tasks: list[ScheduledTask] = []
        self._load()

    def _load(self) -> None:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return
        if isinstance(data, list):
            for d in data:
                if isinstance(d, dict) and d.get("id") and d.get("prompt"):
                    self.tasks.append(ScheduledTask(
                        id=str(d["id"]),
                        prompt=str(d["prompt"]),
                        kind=str(d.get("kind", "once")),
                        next_run=str(d.get("next_run", "")),
                        every_seconds=int(d.get("every_seconds", 0) or 0),
                        at_hour=int(d.get("at_hour", 9) or 0),
                        at_minute=int(d.get("at_minute", 0) or 0),
                        enabled=bool(d.get("enabled", True)),
                        last_run=str(d.get("last_run", "")),
                        runs=int(d.get("runs", 0) or 0),
                        consecutive_failures=int(d.get("consecutive_failures", 0) or 0),
                        allow_desktop=bool(d.get("allow_desktop", False)),
                    ))

    def _save(self) -> None:
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.path.write_text(
                json.dumps([t.to_dict() for t in self.tasks], ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
        except OSError:
            pass  # best-effort; never crash the runtime over scheduling

    def add(self, prompt: str, *, kind: str = "once", run_at: str = "",
            every_seconds: int = 0, at_hour: int = 9, at_minute: int = 0,
            allow_desktop: bool = False,
            now: datetime | None = None) -> ScheduledTask:
        """Create and persist a task. Raises ValueError on invalid input."""
        prompt = (prompt or "").strip()
        if not prompt:
            raise ValueError("prompt is required")
        if kind not in VALID_KINDS:
            raise ValueError(f"kind must be one of {VALID_KINDS}, got {kind!r}")
        if kind == "interval" and int(every_seconds) <= 0:
            raise ValueError("interval tasks need every_seconds > 0")
        if kind == "daily" and not (0 <= int(at_hour) <= 23 and 0 <= int(at_minute) <= 59):
            raise ValueError("daily tasks need a valid at_hour (0-23) and at_minute (0-59)")
        now = now or _now()
        task = ScheduledTask(
            id=uuid.uuid4().hex[:8],
            prompt=prompt, kind=kind, next_run=run_at,
            every_seconds=int(every_seconds), at_hour=int(at_hour), at_minute=int(at_minute),
            allow_desktop=bool(allow_desktop),
        )
        task.next_run = _initial_next_run(task, now)
        self.tasks.append(task)
        self._save()
        return task

    def remove(self, task_id: str) -> bool:
        before = len(self.tasks)
        self.tasks = [t for t in self.tasks if t.id != task_id]
        if len(self.tasks) != before:
            self._save()
            return True
        return False

    def set_enabled(self, task_id: str, enabled: bool) -> bool:
        for t in self.tasks:
            if t.id == task_id:
                t.enabled = enabled
                # Re-enabling clears the failure streak: a task auto-disabled
                # for repeated failures gets a clean slate when the user turns
                # it back on, instead of being disabled again on its next slip.
                if enabled:
                    t.consecutive_failures = 0
                self._save()
                return True
        return False

    def get(self, task_id: str) -> ScheduledTask | None:
        for t in self.tasks:
            if t.id == task_id:
                return t
        return None

    def due(self, now: datetime | None = None) -> list[ScheduledTask]:
        """Enabled tasks whose next_run is at or before *now*."""
        now = now or _now()
        out = []
        for t in self.tasks:
            if not t.enabled:
                continue
            nr = _parse_iso(t.next_run)
            if nr is not None and nr <= now:
                out.append(t)
        return out

    def mark_ran(self, task_id: str, now: datetime | None = None,
                 *, ok: bool = True) -> None:
        """Record a firing and roll the task forward (or disable a spent 'once').

        *ok* reports whether the run succeeded. A run of failures (timeouts /
        errors) with no success in between is counted; once it reaches
        ``MAX_CONSECUTIVE_FAILURES`` the task is auto-disabled so a misconfigured
        recurring task can't spin forever burning tokens every poll. Any success
        resets the counter. A spent ``once`` task is disabled regardless.
        """
        now = now or _now()
        for t in self.tasks:
            if t.id != task_id:
                continue
            t.last_run = now.strftime(_ISO)
            t.runs += 1
            if ok:
                t.consecutive_failures = 0
            else:
                t.consecutive_failures += 1
            nxt = compute_next_run(t, now)
            if nxt is None:
                # A 'once' task is spent: disable it (kept for history).
                t.enabled = False
                t.next_run = ""
            elif t.consecutive_failures >= MAX_CONSECUTIVE_FAILURES:
                # A recurring task that keeps failing is auto-disabled (kept for
                # history). The user can re-enable it after fixing the cause.
                t.enabled = False
                t.next_run = nxt
            else:
                t.next_run = nxt
            self._save()
            return

    def seconds_until_next(self, now: datetime | None = None) -> float | None:
        """Seconds until the soonest enabled task fires, or None if none pending."""
        now = now or _now()
        soonest: datetime | None = None
        for t in self.tasks:
            if not t.enabled:
                continue
            nr = _parse_iso(t.next_run)
            if nr is None:
                continue
            if soonest is None or nr < soonest:
                soonest = nr
        if soonest is None:
            return None
        return max(0.0, (soonest - now).total_seconds())
