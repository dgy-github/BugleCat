"""read_file: line-numbered reads, helpful for weaker models that don't grep well."""

from __future__ import annotations

import mimetypes
from pathlib import Path
from typing import Any

from nanocodex.tools.base import Tool

_MAX_CHARS = 100_000
_DEFAULT_LIMIT = 2000


class ReadFileTool(Tool):
    # Pure read, no approval prompt — safe to run concurrently with other reads.
    read_only = True
    capability_tags = ("read",)

    @property
    def name(self) -> str:
        return "read_file"

    @property
    def description(self) -> str:
        return (
            "Read a UTF-8 text file and return its contents as 'LINE| TEXT'. "
            "Use 'offset' (1-indexed) and 'limit' for large files. Reads are "
            "allowed anywhere readable under the sandbox policy."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path (absolute or workspace-relative)."},
                "offset": {"type": "integer", "minimum": 1, "description": "1-indexed start line (default 1)."},
                "limit": {"type": "integer", "minimum": 1, "description": "Max lines (default 2000)."},
            },
            "required": ["path"],
        }

    def _resolve(self, path: str) -> Path:
        p = Path(path)
        if not p.is_absolute():
            p = self.ctx.workspace / p
        return p.resolve()

    async def execute(self, **kwargs: Any) -> str:
        path = kwargs.get("path")
        if not path or not isinstance(path, str):
            return "Error: 'path' is required and must be a string."
        offset = int(kwargs.get("offset") or 1)
        limit = int(kwargs.get("limit") or _DEFAULT_LIMIT)

        fp = self._resolve(path)
        if not self.ctx.policy.can_read(fp):
            return f"Error: reading {path} is not allowed under the sandbox policy."
        if not fp.exists():
            return f"Error: file not found: {path}"
        if not fp.is_file():
            return f"Error: not a file: {path}"

        try:
            raw = fp.read_bytes()
        except OSError as exc:
            return f"Error reading file: {exc}"
        if not raw:
            return f"(empty file: {path})"

        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            mime = mimetypes.guess_type(str(fp))[0] or "unknown"
            return f"Error: cannot read non-UTF-8 file {path} (type {mime})."

        text = text.replace("\r\n", "\n")
        lines = text.split("\n")
        total = len(lines)
        if offset > total:
            return f"Error: offset {offset} is beyond end of file ({total} lines)."

        start = offset - 1
        end = min(start + limit, total)
        numbered = [f"{start + i + 1}| {ln}" for i, ln in enumerate(lines[start:end])]
        result = "\n".join(numbered)
        if len(result) > _MAX_CHARS:
            result = result[:_MAX_CHARS] + "\n... (truncated)"
        if end < total:
            result += f"\n\n(showing {offset}-{end} of {total} lines; offset={end + 1} to continue)"
        else:
            result += f"\n\n(end of file — {total} lines)"
        return result
