"""record_fact: a research worker writes a confirmed repo fact into state.

Facts carry their source node so the orchestrator's fact_merge step can detect
when two concurrent research nodes disagree and route the conflict to a
needs_clarification node instead of trusting it (see agent/fact_merge.py).
"""

from __future__ import annotations

from typing import Any

from nanocodex.tools.base import Tool


class RecordFactTool(Tool):
    capability_tags = ("fact",)

    @property
    def name(self) -> str:
        return "record_fact"

    @property
    def description(self) -> str:
        return (
            "Record a confirmed fact about the repository or task (something you "
            "VERIFIED, not a guess), so later workers don't re-investigate it. "
            "State the fact in one clear sentence. Only record what you actually "
            "checked; mark uncertainty in the fact text itself."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "fact": {"type": "string", "description": "One confirmed fact, one sentence."},
            },
            "required": ["fact"],
        }

    async def execute(self, **kwargs: Any) -> str:
        from nanocodex.agent.state import Fact

        state = self.ctx.agent_state
        if state is None:
            return "Note: no orchestrated state is active; fact not recorded."
        text = str(kwargs.get("fact", "")).strip()
        if not text:
            return "Error: 'fact' is required."
        node_id = self.ctx.current_node_id or ""
        state.repo_facts.append(Fact(text=text, source_node=node_id))
        return f"Recorded fact ({len(state.repo_facts)} total)."
