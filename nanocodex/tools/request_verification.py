"""request_verification: a worker hands its node off for independent review.

This is the worker's ONLY legitimate path to "done": it cannot mark its own
node done. Calling this flips the node to `verify` and records that a request
exists. The orchestrator's gate (see orchestrator) refuses to mark any code/test
node done unless a verification request was made AND an independent verifier
passed it — so a worker that simply stops without calling this is forced into
fail_with_action rather than silently treated as complete.
"""

from __future__ import annotations

from typing import Any

from nanocodex.tools.base import Tool


class RequestVerificationTool(Tool):
    capability_tags = ("verify_request",)

    @property
    def name(self) -> str:
        return "request_verification"

    @property
    def description(self) -> str:
        return (
            "Submit the current task node for independent verification. Call this "
            "after you have finished the work and written a checkpoint. State "
            "which acceptance criteria you believe are met and exactly how to "
            "check them (commands, files, expected output). You cannot mark a task "
            "done yourself — an independent verifier decides."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "claims": {
                    "type": "array", "items": {"type": "string"},
                    "description": "Acceptance criteria you believe are satisfied.",
                },
                "how_to_check": {
                    "type": "string",
                    "description": "Concrete steps/commands the verifier can run to confirm.",
                },
            },
            "required": ["claims"],
        }

    async def execute(self, **kwargs: Any) -> str:
        state = self.ctx.agent_state
        if state is None:
            return (
                "Note: no orchestrated state is active; verification cannot be "
                "requested outside an orchestrated run."
            )
        node_id = self.ctx.current_node_id or ""
        node = state.node(node_id) if node_id else None
        if node is None:
            return f"Error: current node {node_id!r} not found in the plan."

        claims = [str(x) for x in kwargs.get("claims", []) if str(x).strip()]
        if not claims:
            return "Error: 'claims' must list at least one acceptance criterion."

        node.status = "verify"
        node.outputs["verification_request"] = {
            "claims": claims,
            "how_to_check": str(kwargs.get("how_to_check", "")),
        }
        return (
            f"Node {node_id} submitted for verification with {len(claims)} claim(s). "
            "An independent verifier will now review it."
        )
