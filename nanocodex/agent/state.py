"""Explicit, serializable agent state for the Fable-style orchestrator.

The single-agent loop keeps everything implicit in chat history. The
orchestrator needs the opposite: a structured task graph, checkpoints, and
verifier verdicts that survive a crash and can be reasoned about by plain code
(cycle checks, ready-node selection) instead of by the model.

Every type here is a plain dataclass with explicit ``to_dict`` / ``from_dict``
so persistence is lossless and forward-tolerant (unknown keys are dropped on
load, missing keys fall back to defaults). We do NOT rely on ``asdict`` for the
round trip because load must tolerate older/newer files.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

# --- Task lifecycle -------------------------------------------------------
# Node-level status. `skipped` and `cancelled` exist on purpose (the review
# flagged their absence): a node whose upstream failed is `skipped`, not
# `blocked`; a user stop is `cancelled`, not `failed` (different retry/replan
# handling). `needs_clarification` means "missing information", distinct from
# `blocked` which means "waiting on a dependency".
TASK_STATUSES = (
    "pending",
    "ready",
    "running",
    "verify",
    "done",
    "failed",
    "blocked",
    "needs_clarification",
    "skipped",
    "cancelled",
)

# Terminal statuses never re-enter the scheduler.
TERMINAL_STATUSES = frozenset({"done", "failed", "skipped", "cancelled"})

TASK_KINDS = ("research", "code", "test", "vision", "verify")

# High-level phases the orchestrator moves through.
PHASES = ("scoping", "planning", "execution", "verification", "finalizing")


@dataclass
class TaskNode:
    """One unit of work in the task graph."""

    id: str
    kind: str  # research | code | test | vision | verify
    title: str
    status: str = "pending"
    depends_on: list[str] = field(default_factory=list)
    acceptance: list[str] = field(default_factory=list)
    inputs: dict[str, Any] = field(default_factory=dict)
    outputs: dict[str, Any] = field(default_factory=dict)
    retries: int = 0
    # Per-node retry cap. The review flagged a single GLOBAL retry budget as a
    # foot-gun: one flaky node would exhaust it and block every other node.
    max_retries: int = 2
    assigned_role: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "kind": self.kind,
            "title": self.title,
            "status": self.status,
            "depends_on": list(self.depends_on),
            "acceptance": list(self.acceptance),
            "inputs": dict(self.inputs),
            "outputs": dict(self.outputs),
            "retries": self.retries,
            "max_retries": self.max_retries,
            "assigned_role": self.assigned_role,
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "TaskNode":
        return cls(
            id=str(d["id"]),
            kind=str(d.get("kind", "code")),
            title=str(d.get("title", "")),
            status=str(d.get("status", "pending")),
            depends_on=[str(x) for x in d.get("depends_on", [])],
            acceptance=[str(x) for x in d.get("acceptance", [])],
            inputs=dict(d.get("inputs", {})),
            outputs=dict(d.get("outputs", {})),
            retries=int(d.get("retries", 0)),
            max_retries=int(d.get("max_retries", 2)),
            assigned_role=str(d.get("assigned_role", "")),
        )


@dataclass
class AgentCheckpoint:
    """A durable snapshot written at the end of a node's work.

    The checkpoint — not state.json — is the source of truth on recovery: it is
    written first and atomically, so if the process dies mid-update we can
    reconcile state.json against the checkpoint directory.
    """

    id: str
    node_id: str
    created_at: str
    summary: str
    files_touched: list[str] = field(default_factory=list)
    tests_run: list[str] = field(default_factory=list)
    artifacts: dict[str, str] = field(default_factory=dict)
    open_risks: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "node_id": self.node_id,
            "created_at": self.created_at,
            "summary": self.summary,
            "files_touched": list(self.files_touched),
            "tests_run": list(self.tests_run),
            "artifacts": dict(self.artifacts),
            "open_risks": list(self.open_risks),
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "AgentCheckpoint":
        return cls(
            id=str(d["id"]),
            node_id=str(d.get("node_id", "")),
            created_at=str(d.get("created_at", "")),
            summary=str(d.get("summary", "")),
            files_touched=[str(x) for x in d.get("files_touched", [])],
            tests_run=[str(x) for x in d.get("tests_run", [])],
            artifacts={str(k): str(v) for k, v in d.get("artifacts", {}).items()},
            open_risks=[str(x) for x in d.get("open_risks", [])],
        )


# Verifier verdicts. `fail_with_action` carries corrective next_actions;
# `blocked` means the verifier itself couldn't reach a verdict (e.g. missing
# evidence it cannot gather) and needs human/orchestrator intervention.
VERIFY_STATUSES = ("pass", "fail_with_action", "blocked")


@dataclass
class VerifyResult:
    """A verifier's verdict on one node's acceptance criteria."""

    node_id: str
    status: str  # pass | fail_with_action | blocked
    summary: str = ""
    evidence: list[str] = field(default_factory=list)
    next_actions: list[str] = field(default_factory=list)
    # How many times the verifier had to re-ask itself for a clean JSON verdict
    # before this result. 0 = clean on first try. Persisted so a live run's
    # format-retry cost is observable after the fact (not just via NCX_TRACE).
    reask_count: int = 0

    @property
    def passed(self) -> bool:
        return self.status == "pass"

    def to_dict(self) -> dict[str, Any]:
        return {
            "node_id": self.node_id,
            "status": self.status,
            "summary": self.summary,
            "evidence": list(self.evidence),
            "next_actions": list(self.next_actions),
            "reask_count": self.reask_count,
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "VerifyResult":
        status = str(d.get("status", "")).strip()
        if status not in VERIFY_STATUSES:
            # Conservative default: an unrecognized verdict is NOT a pass.
            status = "fail_with_action"
        return cls(
            node_id=str(d.get("node_id", "")),
            status=status,
            summary=str(d.get("summary", "")),
            evidence=[str(x) for x in d.get("evidence", [])],
            next_actions=[str(x) for x in d.get("next_actions", [])],
            reask_count=int(d.get("reask_count", 0)),
        )


@dataclass
class Fact:
    """A confirmed repo fact, with provenance so conflicts can be reconciled."""

    text: str
    source_node: str = ""
    # When two concurrent research nodes disagree, the merged fact is marked
    # disputed and routed to a needs_clarification node instead of being trusted.
    disputed: bool = False

    def to_dict(self) -> dict[str, Any]:
        return {"text": self.text, "source_node": self.source_node, "disputed": self.disputed}

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Fact":
        return cls(
            text=str(d.get("text", "")),
            source_node=str(d.get("source_node", "")),
            disputed=bool(d.get("disputed", False)),
        )


@dataclass
class AgentState:
    """The whole durable state of one orchestrated goal."""

    goal: str
    mode: str = "orchestrated"  # orchestrated | single_turn
    phase: str = "scoping"
    repo_facts: list[Fact] = field(default_factory=list)
    constraints: list[str] = field(default_factory=list)
    current_plan: list[TaskNode] = field(default_factory=list)
    checkpoints: list[AgentCheckpoint] = field(default_factory=list)
    verify_results: list[VerifyResult] = field(default_factory=list)
    current_focus: str = ""
    acceptance_global: list[str] = field(default_factory=list)
    last_verifier_report: str = ""
    # Global circuit breaker: total number of FAILED nodes tolerated before the
    # whole run aborts. Per-node retries live on TaskNode.max_retries; this is
    # the run-level backstop, not the per-node budget.
    max_failed_nodes: int = 5
    # Monotonic counter used to mint unique node/checkpoint ids without a clock
    # (the workflow/runtime forbids Date.now-style nondeterminism in some paths;
    # a counter keeps id minting deterministic and resume-safe).
    seq: int = 0

    # --- derived views (no persistence) -----------------------------------
    def node(self, node_id: str) -> TaskNode | None:
        for n in self.current_plan:
            if n.id == node_id:
                return n
        return None

    def completed_node_ids(self) -> set[str]:
        return {n.id for n in self.current_plan if n.status == "done"}

    def failed_node_ids(self) -> set[str]:
        return {n.id for n in self.current_plan if n.status == "failed"}

    def latest_checkpoint_for(self, node_id: str) -> AgentCheckpoint | None:
        found = [c for c in self.checkpoints if c.node_id == node_id]
        return found[-1] if found else None

    def latest_verify_for(self, node_id: str) -> VerifyResult | None:
        found = [v for v in self.verify_results if v.node_id == node_id]
        return found[-1] if found else None

    def next_seq(self) -> int:
        self.seq += 1
        return self.seq

    # --- serialization ----------------------------------------------------
    def to_dict(self) -> dict[str, Any]:
        return {
            "goal": self.goal,
            "mode": self.mode,
            "phase": self.phase,
            "repo_facts": [f.to_dict() for f in self.repo_facts],
            "constraints": list(self.constraints),
            "current_plan": [n.to_dict() for n in self.current_plan],
            "checkpoints": [c.to_dict() for c in self.checkpoints],
            "verify_results": [v.to_dict() for v in self.verify_results],
            "current_focus": self.current_focus,
            "acceptance_global": list(self.acceptance_global),
            "last_verifier_report": self.last_verifier_report,
            "max_failed_nodes": self.max_failed_nodes,
            "seq": self.seq,
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "AgentState":
        return cls(
            goal=str(d.get("goal", "")),
            mode=str(d.get("mode", "orchestrated")),
            phase=str(d.get("phase", "scoping")),
            repo_facts=[Fact.from_dict(x) for x in d.get("repo_facts", [])],
            constraints=[str(x) for x in d.get("constraints", [])],
            current_plan=[TaskNode.from_dict(x) for x in d.get("current_plan", [])],
            checkpoints=[AgentCheckpoint.from_dict(x) for x in d.get("checkpoints", [])],
            verify_results=[VerifyResult.from_dict(x) for x in d.get("verify_results", [])],
            current_focus=str(d.get("current_focus", "")),
            acceptance_global=[str(x) for x in d.get("acceptance_global", [])],
            last_verifier_report=str(d.get("last_verifier_report", "")),
            max_failed_nodes=int(d.get("max_failed_nodes", 5)),
            seq=int(d.get("seq", 0)),
        )
