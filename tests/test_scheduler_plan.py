"""Tests for the GUI managed scheduler's pure decision helpers.

These two functions hold the WHOLE security posture of the GUI-hosted scheduler
(Direction A) in Tk-free, clock-free form, so they're unit-tested exhaustively:

* ``_scheduler_run_plan`` maps (allow_desktop, mcp_connected) -> (approver_kind,
  attach_mcp_tools). The safety invariant — a task without allow_desktop gets the
  auto-deny approver AND no desktop tools at all — is locked here.
* ``_format_scheduler_log_entry`` formats the one record an unattended run
  leaves (runs never touch the transcript), with the timestamp injected so it's
  deterministic.
"""

from __future__ import annotations

import asyncio

from nanocodex.gui import (
    _format_schedule_panel_line,
    _format_scheduler_log_entry,
    _hhmm,
    _run_scheduled_turn,
    _scheduler_run_plan,
    _send_button_label,
)

# --- _scheduler_run_plan: the security mapping ----------------------------


def test_allow_desktop_with_mcp_gets_desktop_only_and_tools():
    kind, attach = _scheduler_run_plan(allow_desktop=True, mcp_connected=True)
    assert kind == "desktop_only"
    assert attach is True


def test_allow_desktop_without_mcp_still_desktop_only_but_no_tools():
    # The approver is still the desktop-only one, but there are no live MCP
    # sessions to build tools from, so nothing is attached.
    kind, attach = _scheduler_run_plan(allow_desktop=True, mcp_connected=False)
    assert kind == "desktop_only"
    assert attach is False


def test_no_allow_desktop_gets_auto_deny_and_never_attaches():
    # The default (and safest) path: auto-deny approver, and crucially NO desktop
    # tools attached even if MCP is connected — the task literally cannot drive
    # the desktop. Withholding capability beats relying on the approver to refuse.
    for mcp in (True, False):
        kind, attach = _scheduler_run_plan(allow_desktop=False, mcp_connected=mcp)
        assert kind == "auto_deny", mcp
        assert attach is False, mcp


def test_attach_is_always_false_without_allow_desktop():
    # Restate the invariant as a standalone guard: attach_mcp_tools implies
    # allow_desktop. A future edit that attaches tools to a non-desktop task
    # trips this.
    for ad in (True, False):
        for mcp in (True, False):
            _kind, attach = _scheduler_run_plan(allow_desktop=ad, mcp_connected=mcp)
            if attach:
                assert ad is True, (ad, mcp)


# --- _format_scheduler_log_entry: the unattended-run record ---------------


def test_log_entry_done_no_summary():
    line = _format_scheduler_log_entry(
        now_iso="2026-06-01T09:00:00", task_id="abc123",
        allow_desktop=True, stop_reason="completed",
    )
    assert line == "2026-06-01T09:00:00 [abc123] (desktop) completed"


def test_log_entry_tags_no_desktop():
    line = _format_scheduler_log_entry(
        now_iso="2026-06-01T09:00:00", task_id="t1",
        allow_desktop=False, stop_reason="completed",
    )
    assert "(no-desktop)" in line


def test_log_entry_with_summary():
    line = _format_scheduler_log_entry(
        now_iso="2026-06-01T09:00:00", task_id="t1",
        allow_desktop=True, stop_reason="completed", summary="sent 1 msg",
    )
    assert line.endswith("completed: sent 1 msg")


def test_log_entry_error_wins():
    # An error is the headline; stop_reason/summary are not appended after it.
    line = _format_scheduler_log_entry(
        now_iso="2026-06-01T09:00:00", task_id="t1",
        allow_desktop=True, stop_reason="completed", summary="x",
        error="boom",
    )
    assert "ERROR: boom" in line
    assert "completed" not in line


def test_log_entry_defaults_to_done_when_no_reason():
    line = _format_scheduler_log_entry(
        now_iso="2026-06-01T09:00:00", task_id="t1", allow_desktop=False,
    )
    assert line == "2026-06-01T09:00:00 [t1] (no-desktop) done"


# --- _run_scheduled_turn: lock + two-stage timeout ------------------------
#
# The whole point of this helper is that a stuck unattended task can never pin
# the desktop lock and freeze the GUI. These lock in the four outcomes and the
# ONE safety invariant that matters most: the lock is released on EVERY path.


class _FakeResult:
    """Stand-in for TurnResult (only the attrs the helper reads)."""

    def __init__(self, stop_reason="completed", final_text="all done"):
        self.stop_reason = stop_reason
        self.final_text = final_text


class _FakeLock:
    """threading.Lock-like that records acquire/release for balance checks."""

    def __init__(self, *, available=True):
        self._available = available
        self.acquired = 0
        self.released = 0

    def acquire(self, blocking=True):
        if not self._available:
            return False
        self._available = False
        self.acquired += 1
        return True

    def release(self):
        self._available = True
        self.released += 1


async def test_scheduled_turn_skips_when_lock_unavailable():
    # User is mid-turn (lock held) -> skip this firing, never invoke run, and
    # never touch the lock we didn't get.
    lock = _FakeLock(available=False)
    ran = []

    async def run(_cc):
        ran.append(True)
        return _FakeResult()

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=10)
    assert out["status"] == "skipped"
    assert ran == []
    assert lock.acquired == 0 and lock.released == 0


async def test_scheduled_turn_done_extracts_reason_and_summary():
    lock = _FakeLock()

    async def run(_cc):
        return _FakeResult(stop_reason="completed", final_text="sent 1 message")

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=10)
    assert out["status"] == "done"
    assert out["stop_reason"] == "completed"
    assert out["summary"] == "sent 1 message"
    assert lock.acquired == 1 and lock.released == 1   # balanced


async def test_scheduled_turn_timeout_disabled_runs_unbounded():
    # timeout_s <= 0 disables timing out entirely (still under the lock).
    lock = _FakeLock()

    async def run(_cc):
        await asyncio.sleep(0.01)
        return _FakeResult(stop_reason="completed", final_text="ok")

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=0)
    assert out["status"] == "done"
    assert lock.released == 1


async def test_scheduled_turn_soft_timeout_cancels_cleanly():
    # A cooperating task: it polls the cancel flag and returns once it flips,
    # mimicking the real loop's cancellation path. The soft deadline trips, the
    # turn stops cleanly, and we report a non-forced timeout.
    lock = _FakeLock()

    async def run(cc):
        for _ in range(1000):
            if cc():
                return _FakeResult(stop_reason="cancelled", final_text="")
            await asyncio.sleep(0.005)
        return _FakeResult(stop_reason="completed", final_text="finished early")

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=0.05)
    assert out["status"] == "timeout"
    assert out["forced"] is False          # cooperative cancel returned in time
    assert out["timeout_s"] == 0.05
    assert lock.released == 1


async def test_scheduled_turn_hard_timeout_force_kills():
    # An UNcooperative task: it ignores the cancel flag and hangs, so the hard
    # wait_for ceiling must force-cancel the whole coroutine. This is the case
    # that actually rescues the GUI from a wedged MCP call.
    lock = _FakeLock()

    async def run(_cc):
        await asyncio.sleep(30)            # never honors the cancel flag
        return _FakeResult()

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=0.05,
                                    soft_grace_s=0.05)
    assert out["status"] == "timeout"
    assert out["forced"] is True           # cooperative cancel couldn't return
    assert lock.released == 1              # lock STILL freed after a force-kill


async def test_scheduled_turn_error_releases_lock():
    # run raising must not leak the lock either.
    lock = _FakeLock()

    async def run(_cc):
        raise RuntimeError("kaboom")

    out = await _run_scheduled_turn(lock=lock, run=run, timeout_s=10)
    assert out["status"] == "error"
    assert "kaboom" in out["error"]
    assert lock.acquired == 1 and lock.released == 1   # released despite error


# --- _hhmm: ISO -> HH:MM (sidebar panel display) --------------------------


def test_hhmm_extracts_time():
    assert _hhmm("2026-06-01T14:27:03") == "14:27"


def test_hhmm_tolerates_junk_and_empty():
    assert _hhmm("") == "?"          # nothing -> placeholder, never crashes
    assert _hhmm("nonsense") == "nonsense"   # no 'T' -> echo as-is


# --- _format_schedule_panel_line: the live sidebar read-out ---------------
#
# Pure + clock-free. The glyph encodes live state (* running / - idle / = off),
# is_running is the ONE bit from the scheduler thread, the rest from the store.


def test_panel_line_running_takes_precedence():
    line = _format_schedule_panel_line(
        prompt="watch wechat and reply", enabled=True, kind="interval",
        next_run="2026-06-01T14:27:00", last_run="2026-06-01T14:12:00",
        runs=3, allow_desktop=True, is_running=True,
    )
    assert line.startswith("* ")              # running glyph
    assert "running now" in line
    assert "[desktop]" in line                # allow_desktop badge
    assert "last 14:12" in line and "x3" in line
    # While running we don't advertise a next time (it's happening now).
    assert "next " not in line


def test_panel_line_idle_shows_next():
    line = _format_schedule_panel_line(
        prompt="run tests", enabled=True, kind="daily",
        next_run="2026-06-01T09:00:00", last_run="2026-05-31T09:00:00",
        runs=5, allow_desktop=False, is_running=False,
    )
    assert line.startswith("- ")              # enabled & idle glyph
    assert "next 09:00" in line
    assert "[desktop]" not in line            # no badge when allow_desktop off


def test_panel_line_disabled():
    line = _format_schedule_panel_line(
        prompt="paused task", enabled=False, is_running=False,
    )
    assert line.startswith("= ")              # disabled glyph
    assert "off" in line


def test_panel_line_clips_long_prompt_and_handles_empty():
    long = _format_schedule_panel_line(prompt="x" * 200, enabled=True)
    # The label is clipped (24 chars) so one giant prompt can't blow out the row.
    head = long.splitlines()[0]
    assert len(head) <= 40
    empty = _format_schedule_panel_line(prompt="", enabled=True)
    assert "(empty)" in empty


# --- _send_button_label: Codex-style task-queue surfacing -----------------
#
# Pure so the button text unit-tests without Tk. The label reflects the BACKLOG
# (inputs queued behind the running turn), not whether a turn is running.


def test_send_label_idle_when_nothing_queued():
    assert _send_button_label(queued=0) == "Send  ⏎"


def test_send_label_shows_queue_count():
    assert _send_button_label(queued=1) == "Queue (1)  ⏎"
    assert _send_button_label(queued=3) == "Queue (3)  ⏎"
