"""Role-based tool isolation — enforced in code, not in the prompt.

The review's finding: "planner doesn't edit / verifier can't edit" is worthless
if it lives only in a prompt the model may ignore. Here a role grants a set of
capability tags, and a worker for that role is handed a :class:`ToolRegistry`
containing ONLY the tools whose tags the role grants. A planner literally has no
``apply_patch`` object to call —越权 is impossible, not merely discouraged.

Two layers of enforcement for read-only roles (planner / research / verifier):
1. They are never granted the ``edit`` tag, so ApplyPatchTool is absent.
2. Their ToolContext is rebuilt with a READ_ONLY sandbox policy + matching
   executor, so even the shell they DO get cannot write or reach the network.
"""

from __future__ import annotations

from dataclasses import dataclass, replace

from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import READ_ONLY, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry
from nanocodex.tools.apply_patch import ApplyPatchTool
from nanocodex.tools.base import Tool
from nanocodex.tools.read_file import ReadFileTool
from nanocodex.tools.record_fact import RecordFactTool
from nanocodex.tools.request_verification import RequestVerificationTool
from nanocodex.tools.shell import ShellTool
from nanocodex.tools.update_plan import UpdatePlanTool
from nanocodex.tools.web_search import WebSearchTool
from nanocodex.tools.write_checkpoint import WriteCheckpointTool

# The universe of tool classes the orchestrator can hand out, by role. Note the
# orchestration tools (checkpoint / verify_request / fact) live here but NOT in
# the default single-turn registry — they only make sense inside an orchestrated
# run. Untagged "misc" tools (storyboard, schedule, ...) are deliberately
# omitted from role workers to keep their surface minimal.
ALL_ROLE_TOOL_CLASSES: list[type[Tool]] = [
    ReadFileTool,
    ShellTool,
    ApplyPatchTool,
    UpdatePlanTool,
    WebSearchTool,
    WriteCheckpointTool,
    RequestVerificationTool,
    RecordFactTool,
]


@dataclass(frozen=True)
class Role:
    """A worker role: the capability tags it may use and whether it's read-only."""

    name: str
    allow_tags: frozenset[str]
    read_only: bool = False  # rebuild ctx with a READ_ONLY policy when True

    def grants(self, tool: Tool) -> bool:
        return any(tag in self.allow_tags for tag in tool.capability_tags)


# Role table. Read-only roles never get "edit"; code/test roles do.
ROLES: dict[str, Role] = {
    "planner": Role(
        "planner",
        allow_tags=frozenset({"read", "shell", "plan", "research", "fact"}),
        read_only=True,
    ),
    "research": Role(
        "research",
        allow_tags=frozenset({"read", "shell", "research", "fact"}),
        read_only=True,
    ),
    "code": Role(
        "code",
        allow_tags=frozenset(
            {"read", "shell", "edit", "plan", "checkpoint", "verify_request", "fact"}
        ),
    ),
    "test": Role(
        "test",
        allow_tags=frozenset({"read", "shell", "edit", "checkpoint", "verify_request"}),
    ),
    "vision": Role(
        "vision",
        allow_tags=frozenset(
            {"read", "shell", "research", "checkpoint", "verify_request"}
        ),
    ),
    "verifier": Role(
        "verifier",
        allow_tags=frozenset({"read", "shell", "research"}),
        read_only=True,
    ),
}

# The task kind -> worker role mapping the orchestrator uses to dispatch.
KIND_TO_ROLE = {
    "research": "research",
    "code": "code",
    "test": "test",
    "vision": "vision",
    "verify": "verifier",
}


def _read_only_ctx(ctx: ToolContext) -> ToolContext:
    """A clone of *ctx* whose sandbox forbids writes and network.

    The shell a read-only role receives runs through this policy/executor, so it
    physically cannot write — independent of whatever the model tries.
    """
    ro_policy = SandboxPolicy(mode=READ_ONLY, workspace=ctx.policy.workspace)
    return replace(ctx, policy=ro_policy, executor=make_executor(ro_policy))


def build_role_registry(ctx: ToolContext, role_name: str) -> ToolRegistry:
    """Build a ToolRegistry exposing only the tools *role_name* is allowed.

    Raises KeyError for an unknown role (callers must use a defined role).
    """
    role = ROLES[role_name]
    role_ctx = _read_only_ctx(ctx) if role.read_only else ctx
    allowed: list[type[Tool]] = []
    for cls in ALL_ROLE_TOOL_CLASSES:
        # Instantiate transiently to read capability_tags via the same path the
        # registry uses; cheap and avoids duplicating tag metadata on the class.
        probe = cls(role_ctx)
        if role.grants(probe):
            allowed.append(cls)
    return ToolRegistry(role_ctx, tool_classes=allowed)
