"""shell: run commands under the sandbox + approval state machine.

This is the Codex `shell` / `local_shell` tool. The flow mirrors Codex:

1. Decide via the approval policy whether the command can run automatically,
   must be approved, or is auto-denied.
2. Run it under the sandbox executor.
3. If it fails AND the policy is ``on-failure``, ask to retry unsandboxed
   (here: with a fresh approval prompt). Other policies surface the failure.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from nanocodex.sandbox.approval import ON_FAILURE, ApprovalRequest, Decision, step_decision
from nanocodex.tools.base import Tool


class ShellTool(Tool):
    # "shell" not "write": read-only roles still get the shell, but their
    # ToolContext carries a read-only sandbox policy, so the same tool object
    # physically cannot write/network for them. File EDITING is a separate
    # "edit" capability (apply_patch), which read-only roles never receive.
    capability_tags = ("shell",)

    @property
    def name(self) -> str:
        return "shell"

    @property
    def description(self) -> str:
        return (
            "Run a shell command in the workspace and return its stdout, stderr, "
            "and exit code. Use this to build, run tests, inspect the tree, run "
            "git, etc. Prefer read_file/apply_patch for reading and editing files. "
            "Commands run under a sandbox policy; some require user approval."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to run, as typed in a shell.",
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory (defaults to the workspace root).",
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default from config).",
                    "minimum": 1,
                    "maximum": 600,
                },
                "justification": {
                    "type": "string",
                    "description": (
                        "Why this command is needed. Shown to the user if approval "
                        "is required. Be specific for anything that writes or networks."
                    ),
                },
            },
            "required": ["command"],
        }

    def _resolve_workdir(self, workdir: str | None) -> Path:
        if not workdir:
            return self.ctx.workspace
        p = Path(workdir)
        if not p.is_absolute():
            p = self.ctx.workspace / p
        return p.resolve()

    def _needs_escalation(self, command: str, workdir: Path) -> bool:
        """Heuristic: does this command want something the sandbox forbids?"""
        policy = self.ctx.policy
        if policy.mode == "danger-full-access":
            return False
        # Writing outside the workspace, or any write under read-only.
        if not policy.writes_allowed:
            # read-only: assume anything that isn't plainly read-only escalates.
            return not _looks_read_only(command)
        if not policy.can_write(workdir):
            return True
        return False

    async def execute(self, **kwargs: Any) -> str:
        command = kwargs.get("command")
        if not command or not isinstance(command, str):
            return "Error: 'command' is required and must be a string."
        workdir = self._resolve_workdir(kwargs.get("workdir"))
        timeout = int(kwargs.get("timeout") or self.ctx.timeout_s)
        justification = kwargs.get("justification") or ""

        if not workdir.exists():
            return f"Error: working directory does not exist: {workdir}"

        needs_escalation = self._needs_escalation(command, workdir)
        decision = self.ctx.approver.classify(command, needs_escalation=needs_escalation)
        # In "confirm each step" mode, a shell command always prompts — even an
        # in-sandbox one — so the user can gate the session step by step.
        decision = step_decision(
            decision, is_write=True,
            require_step_approval=getattr(self.ctx, "require_step_approval", False),
        )

        if decision is Decision.AUTO_DENY:
            return (
                "Error: command denied by approval policy 'never' "
                "(it requires escalated permissions). Adjust the approach to stay "
                "within the sandbox, or ask the user to change the policy."
            )
        if decision is Decision.ASK:
            approved = await self.ctx.approver.request(
                ApprovalRequest(
                    command=command,
                    reason=justification or "Command requires approval (per-step confirmation is on).",
                    cwd=str(workdir),
                )
            )
            if not approved:
                return "Error: command not approved by the user."

        result = await self.ctx.executor.run(
            command, cwd=workdir, timeout_s=timeout
        )

        if not result.ok and self.ctx.approver.policy == ON_FAILURE and not result.timed_out:
            approved = await self.ctx.approver.request(
                ApprovalRequest(
                    command=command,
                    reason=(
                        f"Sandboxed run failed (exit {result.exit_code}). "
                        f"{justification}".strip()
                    ),
                    cwd=str(workdir),
                    escalated=True,
                )
            )
            if approved:
                result = await self.ctx.executor.run(
                    command, cwd=workdir, timeout_s=timeout
                )

        return result.render()


_READ_ONLY_PREFIXES = (
    "ls", "cat", "pwd", "echo", "head", "tail", "wc", "grep", "rg", "find",
    "which", "type", "file", "stat", "tree", "git status", "git log", "git diff",
    "git show", "git branch", "dir",
    # NB: `python -c` / `node -e` were intentionally REMOVED — they run arbitrary
    # code (incl. file writes / network) and must never be assumed read-only.
)

# Shell metacharacters that can hide a write even when the leading token looks
# read-only: output redirection (`>`/`>>`/`&>`), command substitution (`$(...)`
# or backticks), and process substitution. Their mere presence disqualifies the
# whole command from the read-only fast-path.
_WRITE_OR_SUBSHELL = (">", "`", "$(", "<(", ">(")

# Operators that chain several commands into one line. We must check EVERY
# segment, not just the leading token — `ls && rm -rf x` starts with `ls` but
# is obviously not read-only.
_CHAIN_SPLIT = re.compile(r"&&|\|\||[;|&\n]")


def _looks_read_only(command: str) -> bool:
    """Best-effort: True only if EVERY chained segment is a known read-only command.

    Conservative on purpose — when in doubt it returns False so the action goes
    through the approval/escalation path rather than silently running. A single
    write/subshell metacharacter, or any segment whose leading token isn't in the
    read-only allowlist, disqualifies the command.
    """
    stripped = command.strip()
    if not stripped:
        return False
    if any(tok in stripped for tok in _WRITE_OR_SUBSHELL):
        return False
    for segment in _CHAIN_SPLIT.split(stripped):
        seg = segment.strip().lower()
        if not seg:
            continue
        if not any(seg == p or seg.startswith(p + " ") or seg.startswith(p + "\t")
                   for p in _READ_ONLY_PREFIXES):
            return False
    return True
