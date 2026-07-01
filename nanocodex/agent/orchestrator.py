"""OrchestratorLoop: plan -> execute -> verify -> replan over a task graph.

This sits ABOVE the existing AgentLoop. AgentLoop stays the worker runtime; the
orchestrator owns planning, role dispatch, the verification gate, retries, and
the run-level circuit breaker. The hard guarantees the review asked for live
here in code:

* A planner-produced graph is validated (DAG, no dangling deps) before it runs;
  an invalid graph is rejected and replanned, never executed.
* A code/test/vision node CANNOT reach `done` unless the worker called
  request_verification AND an independent verifier passed it. A worker that
  simply stops is forced to fail_with_action — the central "fake done" guard.
* Retries are per-node (TaskNode.max_retries); a run-level breaker
  (max_failed_nodes) aborts a run that is failing wholesale.
* Failed nodes propagate `skipped` to their descendants rather than leaving them
  pending forever.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field

from nanocodex.agent.fact_merge import merge_facts
from nanocodex.agent.loop import AgentLoop
from nanocodex.agent.orch_prompts import PLANNER_SYSTEM, ROLE_SYSTEM, node_brief
from nanocodex.agent.roles import KIND_TO_ROLE, build_role_registry
from nanocodex.agent.session import Session
from nanocodex.agent.state import AgentState, TaskNode
from nanocodex.agent.store import AgentStateStore
from nanocodex.agent.task_graph import (
    GraphError,
    propagate_skips,
    ready_nodes,
    validate_graph,
)
from nanocodex.agent.verifier import Verifier, _extract_json_object

# Kinds that must clear the verification gate before they can be marked done.
_GATED_KINDS = frozenset({"code", "test", "vision"})


@dataclass
class OrchestratorResult:
    status: str  # completed | failed | stalled | aborted | plan_failed
    state: AgentState
    summary: str = ""
    failed_nodes: list[str] = field(default_factory=list)


class OrchestratorLoop:
    def __init__(
        self,
        provider,
        base_ctx,  # ToolContext used to build role-scoped registries
        store: AgentStateStore,
        *,
        verifier_provider=None,
        vision_provider=None,
        worker_max_iterations: int = 30,
        verifier_max_iterations: int = 12,
        plan_retries: int = 2,
        cancel_check=None,
    ) -> None:
        self.provider = provider
        self.base_ctx = base_ctx
        self.store = store
        self.verifier_provider = verifier_provider or provider
        self.vision_provider = vision_provider
        self.worker_max_iterations = worker_max_iterations
        self.verifier_max_iterations = verifier_max_iterations
        self.plan_retries = plan_retries
        self.cancel_check = cancel_check

    def _cancelled(self) -> bool:
        return bool(self.cancel_check and self.cancel_check())

    # --- planning ---------------------------------------------------------
    async def _plan(self, state: AgentState) -> bool:
        """Run the planner, validate its graph, retrying on invalid output.

        Returns True if a valid plan is in place. The validation is pure code —
        the model never gets to push an illegal graph (cycle/dangling) into the
        scheduler.
        """
        state.phase = "planning"
        feedback = ""
        for attempt in range(self.plan_retries + 1):
            tools = build_role_registry(self.base_ctx, "planner")
            session = Session(PLANNER_SYSTEM, log_path=None)
            loop = AgentLoop(self.provider, tools, session,
                             max_iterations=self.worker_max_iterations)
            ask = f"Goal: {state.goal}"
            if feedback:
                ask += f"\n\nYour previous plan was rejected: {feedback}\nReturn a corrected JSON plan."
            result = await loop.run_turn(ask, cancel_check=self.cancel_check)
            nodes, err = _parse_plan(result.final_text)
            if err:
                feedback = err
                continue
            try:
                validate_graph(nodes)
            except GraphError as exc:
                feedback = str(exc)
                continue
            state.current_plan = nodes
            return True
        return False

    # --- execution --------------------------------------------------------
    async def run(self, goal: str) -> OrchestratorResult:
        state = AgentState(goal=goal)
        # agent_state rides on the base ctx so checkpoint/fact tools (and any
        # read-only clone of the ctx) mutate THIS run's state.
        self.base_ctx.agent_state = state
        self.store.save(state)

        if not await self._plan(state):
            state.phase = "finalizing"
            self.store.save(state)
            return OrchestratorResult("plan_failed", state,
                                      "Planner could not produce a valid task graph.")

        state.phase = "execution"
        self.store.save(state)
        return await self._execute(state)

    async def _execute(self, state: AgentState) -> OrchestratorResult:
        while True:
            if self._cancelled():
                return self._finish(state, "aborted", "Cancelled by user.")

            propagate_skips(state.current_plan)
            if len(state.failed_node_ids()) > state.max_failed_nodes:
                return self._finish(state, "aborted",
                                    "Circuit breaker: too many failed nodes.")

            ready = ready_nodes(state.current_plan)
            if not ready:
                return self._terminal_result(state)

            # Research nodes are side-effect-free (read-only role, they only
            # record facts) so a ready batch of them runs CONCURRENTLY. Every
            # other kind mutates the workspace and stays serial — a read never
            # races a write, and two code workers never touch files at once.
            research = [n for n in ready if n.kind == "research"]
            others = [n for n in ready if n.kind != "research"]

            if research:
                await asyncio.gather(*(self._run_node(state, n) for n in research))
                self._reconcile_facts(state)
                self.store.save(state)

            for node in others:
                if self._cancelled():
                    return self._finish(state, "aborted", "Cancelled by user.")
                await self._run_node(state, node)
                self.store.save(state)

    async def _run_node(self, state: AgentState, node: TaskNode) -> None:
        state.current_focus = node.id
        node.assigned_role = KIND_TO_ROLE.get(node.kind, "code")
        node.status = "running"

        # A standalone verify-kind node runs the verifier directly (no worker).
        if node.kind == "verify":
            await self._run_verify_node(state, node)
            return

        await self._run_worker(state, node)

        if node.kind == "research":
            # Research has no verification gate; recorded facts ARE its output.
            node.status = "done"
            return

        # Gated kinds: enforce the verification闸门.
        if node.kind in _GATED_KINDS:
            await self._gate_and_verify(state, node)

    def _reconcile_facts(self, state: AgentState) -> None:
        """Merge facts after a concurrent research batch; route conflicts out.

        Conflicting facts are marked disputed (so workers' briefs exclude them)
        and a needs_clarification node is added per conflicting subject. That
        node is non-runnable (its status is not pending/ready), so it surfaces at
        the end as a stall requiring human input rather than silently feeding a
        contradiction into downstream code.
        """
        outcome = merge_facts(state.repo_facts)
        state.repo_facts = outcome.merged
        for subject, facts in outcome.conflicts.items():
            node_id = f"clarify-{subject.replace(' ', '_')[:24]}"
            if state.node(node_id) is not None:
                continue
            values = " | ".join(f.text for f in facts)
            state.current_plan.append(TaskNode(
                id=node_id,
                kind="research",
                title=f"Resolve conflicting facts about: {subject}",
                status="needs_clarification",
                acceptance=[f"Determine the correct value among: {values}"],
                inputs={"conflicting_facts": [f.text for f in facts]},
            ))

    async def _run_worker(self, state: AgentState, node: TaskNode) -> None:
        role = node.assigned_role
        # Set the current node on the base ctx BEFORE building the (possibly
        # cloned) role registry, so checkpoint/verify tools attribute correctly.
        self.base_ctx.current_node_id = node.id
        tools = build_role_registry(self.base_ctx, role)
        system = ROLE_SYSTEM.get(role, ROLE_SYSTEM["code"])
        session = Session(system, log_path=None)  # fresh per node — no bleed
        loop = AgentLoop(self.provider, tools, session,
                         max_iterations=self.worker_max_iterations)
        facts = [f.text for f in state.repo_facts if not f.disputed]
        brief = node_brief(node, facts, state.constraints)
        await loop.run_turn(brief, cancel_check=self.cancel_check)

    async def _gate_and_verify(self, state: AgentState, node: TaskNode) -> None:
        """The anti-"fake done" gate, then independent verification.

        If the worker exited WITHOUT calling request_verification, its node is
        still `running` here (request_verification flips it to `verify`). That is
        the silent-completion path: we refuse to mark it done and treat it as a
        failed verification with a corrective action.
        """
        if node.status != "verify":
            self._apply_failure(
                state, node,
                next_actions=[
                    "You finished without calling request_verification. Re-do the "
                    "node and submit it for verification."
                ],
                summary="Worker did not request verification (no self-declared done).",
            )
            return

        checkpoint = state.latest_checkpoint_for(node.id)
        verifier = Verifier(
            self.verifier_provider,
            build_role_registry(self.base_ctx, "verifier"),
            max_iterations=self.verifier_max_iterations,
            vision_provider=self.vision_provider,
        )
        verdict = await verifier.verify(node, checkpoint, cancel_check=self.cancel_check)
        state.verify_results.append(verdict)
        state.last_verifier_report = verdict.summary

        if verdict.passed:
            node.status = "done"
        elif verdict.status == "blocked":
            node.status = "blocked"
        else:  # fail_with_action
            self._apply_failure(state, node, verdict.next_actions, verdict.summary)

    async def _run_verify_node(self, state: AgentState, node: TaskNode) -> None:
        checkpoint = None
        for dep in node.depends_on:
            checkpoint = state.latest_checkpoint_for(dep) or checkpoint
        verifier = Verifier(
            self.verifier_provider,
            build_role_registry(self.base_ctx, "verifier"),
            max_iterations=self.verifier_max_iterations,
            vision_provider=self.vision_provider,
        )
        verdict = await verifier.verify(node, checkpoint, cancel_check=self.cancel_check)
        state.verify_results.append(verdict)
        node.status = "done" if verdict.passed else (
            "blocked" if verdict.status == "blocked" else "failed"
        )

    def _apply_failure(self, state, node, next_actions, summary) -> None:
        """Retry the node (per-node budget) or mark it failed.

        Corrective actions from the verifier are threaded into the node's inputs
        so the retry worker sees what to fix — a lightweight replan that doesn't
        require re-running the planner for every miss.
        """
        node.retries += 1
        node.inputs["corrective_actions"] = list(next_actions or [])
        if node.retries <= node.max_retries:
            node.status = "pending"  # eligible to run again next wave
        else:
            node.status = "failed"
        node.outputs["last_failure"] = summary

    # --- termination ------------------------------------------------------
    def _terminal_result(self, state: AgentState) -> OrchestratorResult:
        plan = state.current_plan
        failed = [n.id for n in plan if n.status == "failed"]
        blocked = [n.id for n in plan if n.status in ("blocked", "needs_clarification")]
        if failed:
            return self._finish(state, "failed",
                                f"{len(failed)} node(s) failed.", failed)
        if blocked:
            return self._finish(state, "stalled",
                                f"{len(blocked)} node(s) blocked/awaiting clarification.")
        if all(n.status in ("done", "skipped", "cancelled") for n in plan):
            return self._finish(state, "completed", "All nodes complete.")
        return self._finish(state, "stalled", "No runnable nodes remain.")

    def _finish(self, state, status, summary, failed=None) -> OrchestratorResult:
        state.phase = "finalizing"
        self.store.save(state)
        return OrchestratorResult(status, state, summary, failed or [])


def _parse_plan(text: str) -> "tuple[list[TaskNode], str]":
    """Parse a planner's JSON into TaskNodes. Returns (nodes, error_message)."""
    obj = _extract_json_object(text)
    if obj is None:
        return [], "Output was not a single JSON object."
    raw = obj.get("nodes")
    if not isinstance(raw, list) or not raw:
        return [], "JSON had no non-empty 'nodes' array."
    nodes: list[TaskNode] = []
    for item in raw:
        if not isinstance(item, dict) or not item.get("id"):
            return [], "Every node needs an 'id'."
        try:
            nodes.append(TaskNode.from_dict(item))
        except (KeyError, ValueError) as exc:
            return [], f"Bad node: {exc}"
    return nodes, ""
