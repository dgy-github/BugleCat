"""apply_patch tool: the model's primary way to edit files (Codex V4A format)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from nanocodex.sandbox.approval import ApprovalRequest, Decision, step_decision
from nanocodex.tools.base import Tool
from nanocodex.tools.patch import PatchError, apply_patch, parse_patch

_EXAMPLE = (
    "*** Begin Patch\n"
    "*** Update File: src/app.py\n"
    "@@ def main():\n"
    "-    print(\"hi\")\n"
    "+    print(\"hello\")\n"
    "*** End Patch"
)


class ApplyPatchTool(Tool):
    # The file-editing capability. Read-only roles (planner / research /
    # verifier) are never granted "edit", so they cannot get this tool at all.
    capability_tags = ("edit",)

    @property
    def name(self) -> str:
        return "apply_patch"

    @property
    def description(self) -> str:
        return (
            "Create, update, delete, or move files using the V4A patch format. "
            "This is the preferred way to edit code. The patch must be wrapped in "
            "'*** Begin Patch' / '*** End Patch'. Use '*** Add File:', "
            "'*** Update File:', '*** Delete File:', and optional '*** Move to:'. "
            "Inside an Update, prefix context lines with a space, removed lines "
            "with '-', added lines with '+', and use '@@ <context>' to locate the "
            "right spot. Example:\n" + _EXAMPLE
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": (
                        "The full patch text, including the '*** Begin Patch' and "
                        "'*** End Patch' markers."
                    ),
                }
            },
            "required": ["patch"],
        }

    async def execute(self, **kwargs: Any) -> str:
        patch_text = kwargs.get("patch")
        if not patch_text or not isinstance(patch_text, str):
            return "Error: 'patch' is required and must be a string."

        # Parse first so we know which files the patch touches. A parse error
        # is returned directly without involving the approval layer.
        try:
            actions = parse_patch(patch_text)
        except PatchError as exc:
            return f"Error applying patch: {exc}"

        # Find targets that fall outside the writable sandbox. These require
        # approval, mirroring how `shell` escalates out-of-sandbox actions.
        escaping = self._escaping_paths(actions)
        approved_paths: set[Path] = set()
        if escaping:
            decision = self.ctx.approver.classify(
                "apply_patch", needs_escalation=True
            )
            if decision is Decision.AUTO_DENY:
                rel = ", ".join(self._rel(p) for p in escaping)
                return (
                    "Error: patch denied by approval policy 'never' — it writes "
                    f"outside the sandbox ({rel}). Keep changes inside the "
                    "workspace, or ask the user to change the policy."
                )
            if decision is Decision.ASK:
                rel = ", ".join(self._rel(p) for p in escaping)
                approved = await self.ctx.approver.request(
                    ApprovalRequest(
                        command=f"apply_patch writing outside the sandbox: {rel}",
                        reason="The patch modifies files outside the writable roots.",
                        cwd=str(self.ctx.workspace),
                        escalated=True,
                        details=patch_text,
                    )
                )
                if not approved:
                    return "Error: patch not approved by the user."
            approved_paths = set(escaping)
        elif getattr(self.ctx, "require_step_approval", False):
            # In-sandbox write, but per-step confirmation is on: prompt anyway so
            # the user approves each file change before it lands.
            files = ", ".join(self._rel((Path(self.ctx.workspace).resolve() / a.path))
                              for a in actions)
            approved = await self.ctx.approver.request(
                ApprovalRequest(
                    command=f"apply_patch: {files}",
                    reason="Per-step confirmation is on; approve this file change.",
                    cwd=str(self.ctx.workspace),
                    details=patch_text,
                )
            )
            if not approved:
                return "Error: patch not approved by the user."

        def can_write(path: Path) -> bool:
            return self.ctx.policy.can_write(path) or path in approved_paths

        try:
            outcome = apply_patch(
                patch_text,
                root=self.ctx.workspace,
                can_write=can_write,
            )
        except PatchError as exc:
            return f"Error applying patch: {exc}"
        except Exception as exc:  # noqa: BLE001 - reported to the model as a tool error
            return f"Error applying patch: {type(exc).__name__}: {exc}"

        summary = outcome.summary()
        return "Patch applied successfully:\n" + summary if summary else "Patch applied (no changes)."

    def _escaping_paths(self, actions: list) -> list[Path]:
        """Resolved targets the patch would touch that fall outside the sandbox."""
        root = Path(self.ctx.workspace).resolve()
        escaping: list[Path] = []
        seen: set[Path] = set()
        for action in actions:
            rels = [action.path]
            if getattr(action, "move_to", None):
                rels.append(action.move_to)
            for rel in rels:
                target = (root / rel).resolve()
                if target in seen:
                    continue
                seen.add(target)
                if not self.ctx.policy.can_write(target):
                    escaping.append(target)
        return escaping

    def _rel(self, path: Path) -> str:
        try:
            return str(path.relative_to(Path(self.ctx.workspace).resolve()))
        except ValueError:
            return str(path)
