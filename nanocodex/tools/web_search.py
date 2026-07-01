"""web_search: DuckDuckGo search, gated by the sandbox network policy.

Network access mirrors Codex's sandbox semantics:

* ``danger-full-access`` -> network on -> search runs without prompting.
* ``read-only`` / ``workspace-write`` -> network off -> the search is an
  escalation and goes through the approval state machine (ASK / AUTO_DENY /
  AUTO_APPROVE) exactly like an out-of-sandbox shell command.

The search backend is injectable so tests run fully offline.
"""

from __future__ import annotations

import asyncio
from typing import Any, Callable

from nanocodex.sandbox.approval import ApprovalRequest, Decision
from nanocodex.tools.base import Tool

# (query, max_results) -> list of {title, href, body}
SearchFn = Callable[[str, int], list[dict[str, str]]]


def _default_search(query: str, max_results: int) -> list[dict[str, str]]:
    """Run a real DuckDuckGo search via ddgs (lazy import; network required)."""
    from ddgs import DDGS

    with DDGS() as ddgs:
        return list(ddgs.text(query, max_results=max_results))


class WebSearchTool(Tool):
    capability_tags = ("research",)

    def __init__(self, ctx, search_fn: SearchFn | None = None) -> None:
        super().__init__(ctx)
        self._search_fn = search_fn or _default_search

    @property
    def name(self) -> str:
        return "web_search"

    @property
    def description(self) -> str:
        return (
            "Search the web (DuckDuckGo) and return the top results as title + "
            "URL + snippet. Use for current information, docs, or facts outside "
            "the repository. Requires network access; under a no-network sandbox "
            "this asks the user for approval."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The search query."},
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "How many results to return (default 5).",
                },
            },
            "required": ["query"],
        }

    async def execute(self, **kwargs: Any) -> str:
        query = kwargs.get("query")
        if not query or not isinstance(query, str):
            return "Error: 'query' is required and must be a string."
        max_results = int(kwargs.get("max_results") or 5)
        max_results = max(1, min(max_results, 10))

        # Network gating: only consult approval when the sandbox forbids network.
        if not self.ctx.policy.network_access:
            decision = self.ctx.approver.classify(
                f"web_search: {query}", needs_escalation=True
            )
            if decision is Decision.AUTO_DENY:
                return (
                    "Error: web search denied by approval policy 'never' (network "
                    "access is disabled under this sandbox). Ask the user to enable "
                    "network or switch sandbox mode."
                )
            if decision is Decision.ASK:
                approved = await self.ctx.approver.request(
                    ApprovalRequest(
                        command=f"web_search: {query}",
                        reason="Web search requires network access, which the sandbox disables.",
                        cwd=str(self.ctx.workspace),
                        escalated=True,
                    )
                )
                if not approved:
                    return "Error: web search not approved by the user."

        try:
            results = await asyncio.to_thread(self._search_fn, query, max_results)
        except Exception as exc:  # noqa: BLE001 - reported to the model as a tool error
            return f"Error: web search failed: {type(exc).__name__}: {exc}"

        if not results:
            return f"No results for: {query}"

        lines: list[str] = [f"Top {len(results)} results for: {query}"]
        for i, r in enumerate(results, 1):
            title = (r.get("title") or "").strip()
            href = (r.get("href") or r.get("url") or "").strip()
            body = (r.get("body") or "").strip().replace("\n", " ")
            if len(body) > 300:
                body = body[:300] + "…"
            lines.append(f"\n{i}. {title}\n   {href}\n   {body}")
        return "\n".join(lines)
