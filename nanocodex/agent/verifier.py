"""Independent verifier: an isolated review pass over a node's acceptance.

The review's highest-severity finding was that "independent verifier" usually
degrades into a rubber stamp because it shares the worker's conversation and
trusts the worker's narration. This implementation enforces independence in
CODE, not in the prompt:

* Every verification opens a BRAND-NEW :class:`Session`. The worker's messages,
  reasoning, and tool history never enter the verifier's context. The verifier
  sees only: the acceptance criteria, the worker's checkpoint as an INDEX of
  where to look, an optional diff, and its own read-only tools.
* The verifier's tool set is read-only (no apply_patch, no writing shell). The
  caller passes an already-role-scoped registry; the verifier never edits.
* The system prompt instructs it to trust only evidence it gathers itself (its
  own command output / file reads), not the checkpoint's prose, and to fail
  conservatively when evidence is weak.
* A non-JSON or unparseable verdict is treated as ``fail_with_action`` — an
  unreadable verdict is never a pass.
"""

from __future__ import annotations

import json
from typing import Any

from nanocodex.agent.loop import AgentLoop
from nanocodex.agent.session import Session
from nanocodex.agent.state import TaskNode, VerifyResult
from nanocodex.agent.trace import trace

VERIFIER_SYSTEM_PROMPT = """You are an INDEPENDENT verifier in a multi-agent coding system.

You did not write the code under review and you must not trust the worker's
description of what it did. Your job is to decide whether the stated acceptance
criteria are actually satisfied, based ONLY on evidence you gather yourself.

Rules:
- You may read files and run READ-ONLY commands (tests, builds, git diff). You
  CANNOT edit files. The checkpoint you are given is only an index of where to
  look — its prose is NOT evidence. Re-run the checks yourself.
- If you cannot gather the evidence needed to confirm a criterion, do NOT pass
  it. Weak or missing evidence means fail_with_action or blocked.
- Be specific about what you actually observed (command + result, file + line).

When finished, output ONE JSON object and nothing else:
{
  "status": "pass" | "fail_with_action" | "blocked",
  "summary": "one or two sentences",
  "evidence": ["concrete observations you made"],
  "next_actions": ["if not pass, what must change"]
}
- "pass": every acceptance criterion is confirmed by your own evidence.
- "fail_with_action": at least one criterion is unmet; list corrective actions.
- "blocked": you cannot verify (e.g. a required tool/command is unavailable).
"""

# Sent back into the SAME verifier session when its verdict didn't parse. A
# formatting slip is the verifier's problem, not the worker's — re-asking here
# (one cheap model call, full context retained) avoids failing the node and
# re-running the entire worker just because the JSON wasn't clean.
REASK_VERDICT = (
    "Your previous reply was not a single parseable JSON verdict. Reply now with "
    "ONLY the JSON object — no prose, no markdown fence — exactly in this shape:\n"
    '{"status": "pass" | "fail_with_action" | "blocked", "summary": "...", '
    '"evidence": ["..."], "next_actions": ["..."]}'
)


class Verifier:
    """Runs an isolated, read-only verification pass and returns a verdict."""

    def __init__(
        self,
        provider,
        read_only_tools,  # a role-scoped (read-only) ToolRegistry
        *,
        max_iterations: int = 12,
        system_prompt: str = VERIFIER_SYSTEM_PROMPT,
        vision_provider=None,
        format_retries: int = 1,
    ) -> None:
        self.provider = provider
        self.read_only_tools = read_only_tools
        self.max_iterations = max_iterations
        self.system_prompt = system_prompt
        # How many times to re-ask (in the same session) for a clean JSON verdict
        # before giving up. The exhaustion case returns `blocked`, not a worker
        # failure, so a verifier formatting glitch never re-runs the worker.
        self.format_retries = format_retries
        # Optional vision backend for visual acceptance: when a node carries
        # images (inputs["images"]), the verification brief becomes multimodal
        # and that turn routes to the VL model (AgentLoop's per-turn routing).
        self.vision_provider = vision_provider

    def _build_brief(self, node: TaskNode, checkpoint, diff_text: str) -> str:
        """The single user message: criteria + where to look. No worker history."""
        lines: list[str] = []
        lines.append(f"# Verify task node: {node.id} — {node.title}")
        lines.append("")
        lines.append("## Acceptance criteria (each must be confirmed)")
        for i, c in enumerate(node.acceptance, 1):
            lines.append(f"{i}. {c}")
        if not node.acceptance:
            lines.append("(none stated — treat as: the node's stated goal is met)")

        req = (node.outputs or {}).get("verification_request") or {}
        if req.get("claims"):
            lines.append("")
            lines.append("## Worker's claims (verify, do not trust)")
            for c in req["claims"]:
                lines.append(f"- {c}")
        if req.get("how_to_check"):
            lines.append("")
            lines.append("## Worker's suggested checks (a starting point only)")
            lines.append(str(req["how_to_check"]))

        if checkpoint is not None:
            lines.append("")
            lines.append("## Checkpoint index (where to look — NOT evidence)")
            lines.append(f"summary: {checkpoint.summary}")
            if checkpoint.files_touched:
                lines.append(f"files_touched: {', '.join(checkpoint.files_touched)}")
            if checkpoint.tests_run:
                lines.append(f"tests the worker claims it ran: {', '.join(checkpoint.tests_run)}")
            if checkpoint.open_risks:
                lines.append(f"open_risks: {', '.join(checkpoint.open_risks)}")

        if diff_text.strip():
            lines.append("")
            lines.append("## Diff under review")
            lines.append(diff_text.strip())

        lines.append("")
        lines.append(
            "Gather your own evidence, then output the single JSON verdict object."
        )
        return "\n".join(lines)

    async def verify(
        self,
        node: TaskNode,
        checkpoint=None,
        diff_text: str = "",
        *,
        cancel_check=None,
    ) -> VerifyResult:
        # A fresh Session per verification — this is the independence boundary.
        session = Session(self.system_prompt, log_path=None)
        loop = AgentLoop(
            self.provider,
            self.read_only_tools,
            session,
            max_iterations=self.max_iterations,
            vision_provider=self.vision_provider,
        )
        brief = self._build_brief(node, checkpoint, diff_text)
        images = [str(x) for x in (node.inputs or {}).get("images", []) if str(x)]
        if images and self.vision_provider is not None:
            # Multimodal content list -> AgentLoop routes this turn to the VL model.
            content: list[Any] = [{"type": "text", "text": brief}]
            for url in images:
                content.append({"type": "image_url", "image_url": {"url": url}})
            result = await loop.run_turn(content, cancel_check=cancel_check)
        else:
            result = await loop.run_turn(brief, cancel_check=cancel_check)

        # Re-ask in the SAME session if the verdict didn't parse — cheap, keeps
        # the verifier's gathered evidence, and avoids re-running the worker.
        text = result.final_text
        reasks = 0
        for _ in range(self.format_retries):
            if _extract_json_object(text) is not None:
                break
            reasks += 1
            trace("verifier",
                  f"node {node.id}: unparseable verdict, re-asking ({reasks}/{self.format_retries})")
            result = await loop.run_turn(REASK_VERDICT, cancel_check=cancel_check)
            text = result.final_text

        if _extract_json_object(text) is None:
            # Still no clean verdict after re-asking. This is the verifier's
            # formatting failure, not the worker's — return `blocked` (stall this
            # node) rather than fail_with_action (which would re-run the worker).
            trace("verifier",
                  f"node {node.id}: still unparseable after {reasks} re-ask(s) -> blocked")
            return VerifyResult(
                node_id=node.id,
                status="blocked",
                summary="Verifier could not produce a parseable JSON verdict after re-asking.",
                next_actions=["Re-run verification or inspect the node manually."],
                reask_count=reasks,
            )
        verdict = parse_verdict(node.id, text)
        verdict.reask_count = reasks
        trace("verifier",
              f"node {node.id}: verdict={verdict.status} after {reasks} re-ask(s)")
        return verdict


def parse_verdict(node_id: str, text: str) -> VerifyResult:
    """Parse a verifier's final text into a VerifyResult, failing conservatively.

    Tolerates code fences and leading/trailing prose. Any failure to find a
    well-formed verdict object yields ``fail_with_action`` — an unreadable
    verdict is never a pass.
    """
    obj = _extract_json_object(text)
    if obj is None:
        return VerifyResult(
            node_id=node_id,
            status="fail_with_action",
            summary="Verifier did not return a parseable JSON verdict.",
            next_actions=["Re-run verification; ensure a single JSON object is returned."],
        )
    obj.setdefault("node_id", node_id)
    obj["node_id"] = node_id  # never let the model reassign the node
    return VerifyResult.from_dict(obj)


def _extract_json_object(text: str) -> dict[str, Any] | None:
    if not text:
        return None
    s = text.strip()
    # Strip a ```json ... ``` fence if present.
    if s.startswith("```"):
        s = s.split("\n", 1)[-1]
        if s.endswith("```"):
            s = s[: -3]
    # Fast path: the whole thing is JSON.
    try:
        obj = json.loads(s)
        return obj if isinstance(obj, dict) else None
    except json.JSONDecodeError:
        pass
    # Fallback: find the last balanced {...} block (verdict usually comes last).
    start = s.rfind("{")
    while start != -1:
        depth = 0
        for i in range(start, len(s)):
            if s[i] == "{":
                depth += 1
            elif s[i] == "}":
                depth -= 1
                if depth == 0:
                    chunk = s[start : i + 1]
                    try:
                        obj = json.loads(chunk)
                        if isinstance(obj, dict) and "status" in obj:
                            return obj
                    except json.JSONDecodeError:
                        break
        start = s.rfind("{", 0, start)
    return None
