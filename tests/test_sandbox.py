"""Tests for the sandbox policy decisions and the approval state machine."""

from __future__ import annotations

from pathlib import Path

from nanocodex.sandbox.approval import (
    NEVER,
    ON_FAILURE,
    ON_REQUEST,
    UNTRUSTED,
    Approver,
    Decision,
)
from nanocodex.sandbox.policy import (
    DANGER_FULL_ACCESS,
    READ_ONLY,
    WORKSPACE_WRITE,
    SandboxPolicy,
)

# --- policy ---------------------------------------------------------------


def test_read_only_forbids_writes(tmp_path):
    policy = SandboxPolicy(mode=READ_ONLY, workspace=tmp_path)
    assert policy.can_read(tmp_path / "f.py") is True
    assert policy.can_write(tmp_path / "f.py") is False
    assert policy.writes_allowed is False


def test_workspace_write_allows_inside_only(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    # Disable temp-write so the boundary check isn't masked by tmp_path itself
    # living under the system temp dir (which workspace-write allows by design).
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws, allow_temp_write=False)
    assert policy.can_write(ws / "a.py") is True
    assert policy.can_write(ws / "sub" / "b.py") is True
    assert policy.can_write(outside / "c.py") is False


def test_workspace_write_denies_system_temp_by_default(tmp_path):
    # Tightened sandbox: workspace-write does NOT allow the system temp dir
    # unless allow_temp_write is explicitly enabled.
    import tempfile

    ws = tmp_path / "ws"
    ws.mkdir()
    tmp_file = Path(tempfile.gettempdir()) / "nanocodex_probe.txt"

    default_policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws)
    assert default_policy.can_write(tmp_file) is False  # denied by default now

    opted_in = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws, allow_temp_write=True)
    assert opted_in.can_write(tmp_file) is True          # still works when opted in


def test_workspace_write_honors_extra_writable_roots(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    extra = tmp_path / "extra"
    extra.mkdir()
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws, writable_roots=[extra])
    assert policy.can_write(extra / "x.py") is True


def test_danger_full_access_allows_everything(tmp_path):
    policy = SandboxPolicy(mode=DANGER_FULL_ACCESS, workspace=tmp_path)
    assert policy.can_write("/etc/passwd") is True


# --- approval -------------------------------------------------------------


async def _yes(_req) -> bool:
    return True


def test_never_policy_auto_denies_escalation():
    approver = Approver(NEVER, _yes)
    assert approver.classify("rm -rf /", needs_escalation=True) is Decision.AUTO_DENY
    assert approver.classify("ls", needs_escalation=False) is Decision.AUTO_APPROVE


def test_on_request_asks_only_on_escalation():
    approver = Approver(ON_REQUEST, _yes)
    assert approver.classify("ls", needs_escalation=False) is Decision.AUTO_APPROVE
    assert approver.classify("curl example.com", needs_escalation=True) is Decision.ASK


def test_on_failure_runs_first():
    approver = Approver(ON_FAILURE, _yes)
    # on-failure always runs first; approval only sought after a failure.
    assert approver.classify("anything", needs_escalation=True) is Decision.AUTO_APPROVE


def test_untrusted_auto_approves_safe_commands():
    approver = Approver(UNTRUSTED, _yes)
    assert approver.classify("ls -la", needs_escalation=False) is Decision.AUTO_APPROVE
    assert approver.classify("git status", needs_escalation=False) is Decision.AUTO_APPROVE
    assert approver.classify("cat file.txt", needs_escalation=False) is Decision.AUTO_APPROVE


def test_untrusted_asks_for_unknown_or_write_commands():
    approver = Approver(UNTRUSTED, _yes)
    assert approver.classify("npm install", needs_escalation=False) is Decision.ASK
    assert approver.classify("git push", needs_escalation=False) is Decision.ASK
    assert approver.classify("rm -rf build", needs_escalation=False) is Decision.ASK


def test_untrusted_blocks_dangerous_even_if_leading_token_trusted():
    approver = Approver(UNTRUSTED, _yes)
    # 'git clean -fd' leads with trusted 'git' but is a write subcommand.
    assert approver.classify("git clean -fd", needs_escalation=False) is Decision.ASK


async def test_request_invokes_callback():
    calls: list = []

    async def cb(req) -> bool:
        calls.append(req.command)
        return False

    approver = Approver(ON_REQUEST, cb)
    from nanocodex.sandbox.approval import ApprovalRequest

    approved = await approver.request(ApprovalRequest(command="x", reason="r", cwd="/tmp"))
    assert approved is False
    assert calls == ["x"]
