"""Tool registry: build the Codex tool set and dispatch calls."""

from __future__ import annotations

import os
from typing import Any

from nanocodex.tools.apply_patch import ApplyPatchTool
from nanocodex.tools.base import Tool, ToolContext
from nanocodex.tools.read_file import ReadFileTool
from nanocodex.tools.remember_tool import RememberTool
from nanocodex.tools.schedule_tool import ManageScheduleTool
from nanocodex.tools.shell import ShellTool
from nanocodex.tools.skills_tool import ManageSkillsTool
from nanocodex.tools.storyboard_tool import StoryboardTool
from nanocodex.tools.update_plan import UpdatePlanTool, render_plan
from nanocodex.tools.web_search import WebSearchTool

# Order here is the order the model sees the tools.
_DEFAULT_TOOL_CLASSES: list[type[Tool]] = [
    ShellTool,
    ApplyPatchTool,
    UpdatePlanTool,
    ReadFileTool,
    WebSearchTool,
    ManageScheduleTool,
    ManageSkillsTool,
    RememberTool,
    StoryboardTool,
]


TOOL_ALLOW_ENV = "NCX_TOOL_ALLOW"
TOOL_DENY_ENV = "NCX_TOOL_DENY"


def _parse_csv_set(raw: str | None) -> set[str]:
    if not raw:
        return set()
    return {item.strip() for item in raw.split(",") if item.strip()}


def tool_allowed_by(
    name: str,
    *,
    allow: set[str] | None = None,
    deny: set[str] | None = None,
) -> bool:
    """True when *name* passes optional allow/deny filters (deny always wins)."""
    if deny and name in deny:
        return False
    if allow is not None and name not in allow:
        return False
    return True


class ToolRegistry:
    def __init__(
        self,
        ctx: ToolContext,
        tool_classes: "list[type[Tool]] | None" = None,
        *,
        allow_tools: set[str] | None = None,
        deny_tools: set[str] | None = None,
    ) -> None:
        self.ctx = ctx
        env_allow = _parse_csv_set(os.environ.get(TOOL_ALLOW_ENV))
        env_deny = _parse_csv_set(os.environ.get(TOOL_DENY_ENV))
        if allow_tools is not None and env_allow:
            self._allow: set[str] | None = set(allow_tools) & env_allow
        elif allow_tools is not None:
            self._allow = set(allow_tools)
        elif env_allow:
            self._allow = env_allow
        else:
            self._allow = None
        self._deny: set[str] = set(deny_tools or ()) | env_deny
        self._tools: dict[str, Tool] = {}
        for cls in tool_classes if tool_classes is not None else _DEFAULT_TOOL_CLASSES:
            tool = cls(ctx)
            if self._is_allowed(tool.name):
                self._tools[tool.name] = tool

    def _is_allowed(self, name: str) -> bool:
        return tool_allowed_by(name, allow=self._allow, deny=self._deny)

    def register(self, tool: Tool) -> None:
        if self._is_allowed(tool.name):
            self._tools[tool.name] = tool

    def get(self, name: str) -> Tool | None:
        return self._tools.get(name)

    @property
    def names(self) -> list[str]:
        return list(self._tools)

    def schemas(self) -> list[dict[str, Any]]:
        return [t.to_schema() for t in self._tools.values()]

    async def execute(self, name: str, arguments: dict[str, Any]) -> str:
        tool = self._tools.get(name)
        if tool is None:
            return f"Error: unknown tool '{name}'. Available: {', '.join(self.names)}."
        if not isinstance(arguments, dict):
            return f"Error: arguments for '{name}' must be an object."
        try:
            return await tool.execute(**arguments)
        except TypeError as exc:
            return f"Error: bad arguments for '{name}': {exc}"
        except Exception as exc:  # noqa: BLE001 - surfaced to the model
            return f"Error executing '{name}': {type(exc).__name__}: {exc}"


__all__ = ["ToolRegistry", "ToolContext", "Tool", "render_plan"]
