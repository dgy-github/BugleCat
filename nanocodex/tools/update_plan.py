"""update_plan: Codex's planning tool.

The model maintains a short checklist of steps with statuses. nanocodex stores
the latest plan on the shared :class:`ToolContext` so the CLI can render it.
Mirrors Codex semantics: exactly one step should be ``in_progress`` at a time.
"""

from __future__ import annotations

from typing import Any

from nanocodex.tools.base import Tool

_VALID_STATUS = ("pending", "in_progress", "completed")


class UpdatePlanTool(Tool):
    capability_tags = ("plan",)

    @property
    def name(self) -> str:
        return "update_plan"

    @property
    def description(self) -> str:
        return (
            "Create or update a short step-by-step plan for the current task. "
            "Call this at the start of a multi-step task and whenever a step's "
            "status changes. Each step has a 'step' description and a 'status' of "
            "'pending', 'in_progress', or 'completed'. Keep exactly one step "
            "'in_progress'. Use an 'explanation' to note any change of plan."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "explanation": {
                    "type": "string",
                    "description": "Optional note about why the plan changed.",
                },
                "plan": {
                    "type": "array",
                    "description": "The full plan; replaces any previous plan.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": {"type": "string"},
                            "status": {
                                "type": "string",
                                "enum": list(_VALID_STATUS),
                            },
                        },
                        "required": ["step", "status"],
                    },
                },
            },
            "required": ["plan"],
        }

    async def execute(self, **kwargs: Any) -> str:
        plan = kwargs.get("plan")
        if not isinstance(plan, list) or not plan:
            return "Error: 'plan' must be a non-empty array of {step, status}."

        normalized: list[dict[str, str]] = []
        for item in plan:
            if not isinstance(item, dict):
                return "Error: each plan item must be an object with 'step' and 'status'."
            step = str(item.get("step", "")).strip()
            status = str(item.get("status", "")).strip()
            if not step:
                return "Error: a plan item is missing 'step'."
            if status not in _VALID_STATUS:
                return f"Error: invalid status {status!r}; expected one of {_VALID_STATUS}."
            normalized.append({"step": step, "status": status})

        in_progress = [s for s in normalized if s["status"] == "in_progress"]
        if len(in_progress) > 1:
            return "Error: keep exactly one step 'in_progress' at a time."

        if self.ctx.plan is None:
            self.ctx.plan = []
        self.ctx.plan.clear()
        self.ctx.plan.extend(normalized)

        return "Plan updated:\n" + render_plan(normalized)


def render_plan(plan: list[dict[str, str]]) -> str:
    marks = {"completed": "[x]", "in_progress": "[~]", "pending": "[ ]"}
    return "\n".join(f"  {marks.get(s['status'], '[ ]')} {s['step']}" for s in plan)
