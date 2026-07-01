"""Codex-style approval state machine.

Mirrors Codex's four approval policies:

    untrusted    auto-run only known-safe (read-only-ish) commands; ask for the rest
    on-failure   run sandboxed first; if it fails, ask to retry unsandboxed
    on-request   model decides when to ask; escalation requests are honored (default)
    never        never ask; anything needing approval is denied and reported

The agent loop consults :class:`Approver` before running a shell command and
again (with ``on_failure=True``) if a sandboxed run fails. The actual yes/no
comes from an injected callback so the CLI can prompt the console while tests
can pass a scripted decision.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from enum import Enum
from typing import Awaitable, Callable

UNTRUSTED = "untrusted"
ON_FAILURE = "on-failure"
ON_REQUEST = "on-request"
NEVER = "never"


class Decision(Enum):
    AUTO_APPROVE = "auto_approve"  # run, sandboxed
    ASK = "ask"                    # must prompt the user
    AUTO_DENY = "auto_deny"        # refuse without asking (policy=never)


# Tools that MODIFY state. Under "confirm each step" mode these always prompt,
# even for in-sandbox actions, so the user controls the session's pace.
WRITE_TOOLS = frozenset({"shell", "apply_patch"})


def step_decision(base: "Decision", *, is_write: bool, require_step_approval: bool) -> "Decision":
    """Layer per-step confirmation on top of the sandbox-escalation decision.

    The base decision comes from :meth:`Approver.classify` (escalation/policy).
    When the user has turned OFF auto-approve (``require_step_approval`` True),
    any WRITE action that would otherwise auto-run is upgraded to ASK — this is
    what makes the GUI toggle actually gate the session. AUTO_DENY is never
    softened, and an existing ASK stays ASK.
    """
    if base is Decision.AUTO_DENY:
        return base
    if require_step_approval and is_write and base is Decision.AUTO_APPROVE:
        return Decision.ASK
    return base


# Commands considered safe to run without approval under `untrusted`.
# Conservative, read-only-ish allowlist (first token of the command).
_TRUSTED_COMMANDS = frozenset({
    "ls", "cat", "pwd", "echo", "head", "tail", "wc", "grep", "rg", "find",
    "which", "type", "file", "stat", "tree", "date", "whoami", "env", "printenv",
    "git", "python", "python3", "node", "pytest", "ruff", "true", "false",
    "dir", "where",
})
# git subcommands that are NOT read-only -> still need approval under untrusted.
_GIT_WRITE_SUBCMDS = frozenset({
    "push", "commit", "reset", "rebase", "merge", "clean", "checkout",
    "branch", "tag", "stash", "rm", "mv", "cherry-pick", "revert", "am",
})

# Patterns that always require approval regardless of the leading token.
_DANGEROUS_PATTERNS = (
    re.compile(r"\brm\s+-[rf]"),
    re.compile(r"\bdel\s+/[fq]"),
    re.compile(r"\brmdir\s+/s"),
    re.compile(r"(?:^|[;&|]\s*)format\b"),
    re.compile(r"\b(mkfs|diskpart|dd)\b"),
    re.compile(r"\b(shutdown|reboot|poweroff)\b"),
    re.compile(r":\(\)\s*\{.*\};\s*:"),
)


@dataclass
class ApprovalRequest:
    """Context handed to the approval callback for a human decision."""

    command: str
    reason: str
    cwd: str
    escalated: bool = False
    # Optional extra context for the human, shown verbatim below the command —
    # e.g. apply_patch passes the full patch (a diff) so the user reviews the
    # actual change before approving, not just the file names. Empty = nothing
    # extra to show.
    details: str = ""


# (request) -> approved?
ApprovalCallback = Callable[[ApprovalRequest], Awaitable[bool]]


def _first_token(command: str) -> str:
    try:
        parts = shlex.split(command, posix=False)
    except ValueError:
        parts = command.strip().split()
    return parts[0].lower() if parts else ""


def _is_trusted(command: str) -> bool:
    if any(p.search(command) for p in _DANGEROUS_PATTERNS):
        return False
    head = _first_token(command)
    base = head.rsplit("/", 1)[-1].rsplit("\\", 1)[-1]
    base = base[:-4] if base.endswith(".exe") else base
    if base not in _TRUSTED_COMMANDS:
        return False
    if base == "git":
        try:
            tokens = shlex.split(command, posix=False)
        except ValueError:
            tokens = command.split()
        sub = next((t for t in tokens[1:] if not t.startswith("-")), "")
        if sub.lower() in _GIT_WRITE_SUBCMDS:
            return False
    return True


class Approver:
    """Decide whether a shell command may run, and prompt when required."""

    def __init__(self, policy: str, callback: ApprovalCallback) -> None:
        self.policy = policy
        self._callback = callback

    def classify(self, command: str, *, needs_escalation: bool) -> Decision:
        """Pure decision: can this run automatically, must we ask, or auto-deny?

        *needs_escalation* is True when the command wants something the sandbox
        forbids (e.g. writing outside the workspace, or network access).
        """
        if self.policy == NEVER:
            if needs_escalation:
                return Decision.AUTO_DENY
            return Decision.AUTO_APPROVE
        if self.policy == ON_REQUEST:
            return Decision.ASK if needs_escalation else Decision.AUTO_APPROVE
        if self.policy == ON_FAILURE:
            # Run first; approval is only sought after a sandboxed failure.
            return Decision.AUTO_APPROVE
        if self.policy == UNTRUSTED:
            if _is_trusted(command) and not needs_escalation:
                return Decision.AUTO_APPROVE
            return Decision.ASK
        return Decision.ASK

    async def request(self, req: ApprovalRequest) -> bool:
        """Ask the human (via the injected callback). Returns approval."""
        return await self._callback(req)
