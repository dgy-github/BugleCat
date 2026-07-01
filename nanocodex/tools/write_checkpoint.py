"""write_checkpoint: a worker records a durable snapshot of what it just did.

The checkpoint is the evidence index the verifier reads from and the source of
truth recovery reconciles against. The worker calls this AFTER finishing its
node's work and BEFORE requesting verification. It mutates the orchestrator's
AgentState on the shared ToolContext; outside an orchestrated run (no
agent_state) it is a no-op that tells the model so.
"""

from __future__ import annotations

from typing import Any

from nanocodex.tools.base import Tool


class WriteCheckpointTool(Tool):
    capability_tags = ("checkpoint",)

    @property
    def name(self) -> str:
        return "write_checkpoint"

    @property
    def description(self) -> str:
        return (
            "Record a checkpoint summarizing the work you just completed for the "
            "current task node: a short summary, the files you changed, the tests "
            "you ran (commands), any artifacts, and open risks. Call this once "
            "after finishing the node and before requesting verification. Do not "
            "claim a task is done here — verification decides that."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "summary": {"type": "string", "description": "What you did, in 1-3 sentences."},
                "files_touched": {
                    "type": "array", "items": {"type": "string"},
                    "description": "Paths you created/edited/deleted.",
                },
                "tests_run": {
                    "type": "array", "items": {"type": "string"},
                    "description": "Test/verification commands you actually ran.",
                },
                "open_risks": {
                    "type": "array", "items": {"type": "string"},
                    "description": "Known risks or things left unverified.",
                },
            },
            "required": ["summary"],
        }

    async def execute(self, **kwargs: Any) -> str:
        # Imported lazily to keep tools/base free of an agent-layer import cycle.
        from nanocodex.agent.store import AgentStateStore

        state = self.ctx.agent_state
        if state is None:
            return (
                "Note: no orchestrated state is active, so this checkpoint was not "
                "persisted. (write_checkpoint only applies inside an orchestrated run.)"
            )
        node_id = self.ctx.current_node_id or ""
        summary = str(kwargs.get("summary", "")).strip()
        if not summary:
            return "Error: 'summary' is required."

        store = AgentStateStore(self.ctx.workspace / ".nanocodex")
        ckpt = store.new_checkpoint(
            state, node_id, summary,
            files_touched=[str(x) for x in kwargs.get("files_touched", [])],
            tests_run=[str(x) for x in kwargs.get("tests_run", [])],
            open_risks=[str(x) for x in kwargs.get("open_risks", [])],
        )
        # Checkpoint hits disk FIRST (atomic), then it is recorded into state and
        # state is saved — the write order the recovery logic depends on.
        store.write_checkpoint(ckpt)
        state.checkpoints.append(ckpt)
        store.save(state)
        return f"Checkpoint {ckpt.id} recorded for node {node_id or '(unknown)'}."
