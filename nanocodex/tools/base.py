"""Tool base classes and the execution context shared across tools."""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from nanocodex.sandbox.approval import Approver
from nanocodex.sandbox.executor import PolicyExecutor
from nanocodex.sandbox.policy import SandboxPolicy


@dataclass
class ToolContext:
    """Everything a tool needs to do its job, injected at registration."""

    workspace: Path
    policy: SandboxPolicy
    approver: Approver
    executor: PolicyExecutor
    timeout_s: int = 120
    # Shared mutable plan state for update_plan / the CLI to read.
    plan: list[dict[str, str]] | None = None
    # The orchestrator's explicit AgentState, injected when a tool call runs
    # inside an orchestrated worker. write_checkpoint / request_verification /
    # record_fact mutate it; plain single-turn runs leave it None. Typed as Any
    # to avoid a base-layer import cycle with nanocodex.agent.state.
    agent_state: Any = None
    # The id of the task node the current worker is executing, so checkpoint /
    # verification tools attribute their writes to the right node without the
    # model having to pass it. Set per-node by the orchestrator.
    current_node_id: str | None = None
    # When True, write actions (shell / apply_patch) prompt for approval on
    # EVERY step — even inside the sandbox. This is the "confirm each step"
    # mode the GUI's auto-approve toggle flips (auto-approve OFF -> True). It's
    # a plain bool the worker thread can read/flip safely (atomic in CPython).
    require_step_approval: bool = False
    # Running total of Seedance video spend (CNY) for this session. The
    # StoryboardTool adds each render's cost here so the GUI can show it in the
    # status bar. Kept separate from the USD turn cost (no FX rate is invented):
    # Seedance bills in CNY on a different axis than the text models.
    seedance_cost_cny: float = 0.0


class Tool(ABC):
    """An agent capability exposed to the model as an OpenAI function tool."""

    # True for tools that only READ (no file/network/state side effects) and
    # never prompt for approval. The agent loop runs a run of consecutive
    # read-only tool calls CONCURRENTLY (a write/unknown tool stays serial and
    # ordered). Default False — a tool must explicitly opt in. MCP and other
    # tools of unknown behavior keep the safe default.
    read_only: bool = False

    # Capability tags used by role-based tool isolation (see agent/roles.py).
    # A role grants a set of tags; a tool is exposed to a worker only if at
    # least one of its tags is granted. Default () means "untagged" — untagged
    # tools are only available to roles that explicitly allow untagged tools
    # (the full/default role), so a planner physically cannot be handed a writer
    # tool. Examples: ("read",), ("write",), ("plan",), ("verify_control",).
    capability_tags: tuple[str, ...] = ()

    def __init__(self, ctx: ToolContext) -> None:
        self.ctx = ctx

    @property
    @abstractmethod
    def name(self) -> str: ...

    @property
    @abstractmethod
    def description(self) -> str: ...

    @property
    @abstractmethod
    def parameters(self) -> dict[str, Any]:
        """JSON Schema for the arguments object."""

    @abstractmethod
    async def execute(self, **kwargs: Any) -> str:
        """Run the tool and return a string result for the model."""

    def to_schema(self) -> dict[str, Any]:
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        }
