"""Expand ``@path`` file mentions in a user prompt into inline file context.

Codex-style: typing ``@src/foo.py`` in a message pulls that file's contents into
the turn so the model sees it without a separate read_file round-trip. Pure and
offline: given the raw text and a workspace root, returns the text with a context
block appended for each @mention that resolves to a readable UTF-8 file. A @token
that doesn't resolve to a file is left untouched, so ``@channel``, e-mail
addresses, and Python decorators are never mangled.
"""

from __future__ import annotations

import re
from pathlib import Path

# '@' at line start or after whitespace, then a non-space run. Trailing sentence
# punctuation is trimmed off the captured path so "see @foo.py." resolves foo.py.
_MENTION = re.compile(r"(?:^|(?<=\s))@([^\s@]+)")
_TRIM_TRAILING = ".,;:!?)]}'\"`"

# Caps so a careless @mention (or many) can't blow the context window.
_MAX_FILE_BYTES = 50_000
_MAX_FILES = 10
_MAX_TOTAL_BYTES = 200_000


def find_mentions(text: str) -> list[str]:
    """Return the @-mention path tokens in order (trailing punctuation trimmed)."""
    out: list[str] = []
    for m in _MENTION.finditer(text or ""):
        tok = m.group(1).rstrip(_TRIM_TRAILING)
        if tok:
            out.append(tok)
    return out


def expand_file_mentions(text: str, workspace: "Path | str") -> str:
    """Append inline file context for each @mention that resolves to a readable file.

    The original text is preserved (the @mention stays inline so the model can
    correlate it); one fenced ``<file path="…">`` block per resolved file is
    appended after it. Files are de-duplicated and capped in count and total
    size; paths resolve relative to *workspace* (absolute paths are read as-is).
    Mentions that don't resolve to a readable UTF-8 file are ignored. Returns the
    text unchanged when nothing resolves.
    """
    workspace = Path(workspace)
    seen: set[Path] = set()
    blocks: list[str] = []
    total = 0
    for tok in find_mentions(text):
        if len(seen) >= _MAX_FILES:
            break
        p = Path(tok)
        if not p.is_absolute():
            p = workspace / tok
        try:
            p = p.resolve()
        except OSError:
            continue
        if p in seen or not p.is_file():
            continue
        try:
            data = p.read_bytes()
        except OSError:
            continue
        if not data:
            continue
        truncated = len(data) > _MAX_FILE_BYTES
        if truncated:
            data = data[:_MAX_FILE_BYTES]
        if total + len(data) > _MAX_TOTAL_BYTES:
            break
        try:
            content = data.decode("utf-8")
        except UnicodeDecodeError:
            continue  # skip binary / non-text files
        seen.add(p)
        total += len(data)
        suffix = "\n... (truncated)" if truncated else ""
        blocks.append(f'<file path="{tok}">\n{content}{suffix}\n</file>')
    if not blocks:
        return text
    return text + "\n\n" + "\n\n".join(blocks)
