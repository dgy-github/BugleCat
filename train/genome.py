#!/usr/bin/env python3
"""Genome model for ncx-forge (M0b).

A genome is the evolvable harness surface: the base system_prompt + per-tool
descriptions. The baseline is extracted from the agent itself via
`ncx --dump-genome` (never by parsing Rust source), so it always reflects the
real tool list and the real, load-bearing descriptions.

Pure stdlib: tomllib (read) + a tiny matching writer (no tomli_w dependency,
no dependency on the legacy nanocodex python package).
"""
from __future__ import annotations

import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent / "bench"
sys.path.insert(0, str(BENCH))
import run as bench  # noqa: E402  -> bench.NCX
from process_control import run_owned  # noqa: E402

# Headroom over the measured baseline so a faithful (e.g. apply_patch) description
# is never rejected, while still bounding teacher-introduced bloat.
SIZE_CAP_MULTIPLIER = 3
SIZE_CAP_FLOOR = 12_000


def _toml_escape(s: str) -> str:
    out = []
    for c in s:
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c == "\n":
            out.append("\\n")
        elif c == "\r":
            out.append("\\r")
        elif c == "\t":
            out.append("\\t")
        else:
            out.append(c)
    return "".join(out)


@dataclass
class Genome:
    system_prompt: str = ""
    tool_desc: dict[str, str] = field(default_factory=dict)

    def to_toml(self) -> str:
        lines = [f'system_prompt = "{_toml_escape(self.system_prompt)}"', "", "[tool_desc]"]
        for name in sorted(self.tool_desc):
            lines.append(f'{name} = "{_toml_escape(self.tool_desc[name])}"')
        return "\n".join(lines) + "\n"

    @classmethod
    def from_toml(cls, text: str) -> "Genome":
        d = tomllib.loads(text)
        return cls(
            system_prompt=str(d.get("system_prompt", "")),
            tool_desc={k: str(v) for k, v in (d.get("tool_desc") or {}).items()},
        )

    def save(self, path: Path) -> None:
        path.write_text(self.to_toml(), encoding="utf-8")

    @classmethod
    def load(cls, path: Path) -> "Genome":
        return cls.from_toml(Path(path).read_text(encoding="utf-8"))

    def copy(self) -> "Genome":
        return Genome(self.system_prompt, dict(self.tool_desc))


def extract_current() -> Genome:
    """Run `ncx --dump-genome` and parse the default genome."""
    r = run_owned(
        [str(bench.NCX), "--dump-genome"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60,
    )
    if r.returncode != 0 or not r.stdout.strip():
        raise RuntimeError(f"ncx --dump-genome failed (rc={r.returncode}): {r.stderr[:200]}")
    return Genome.from_toml(r.stdout)


def _field_cap(baseline_len: int) -> int:
    return max(baseline_len * SIZE_CAP_MULTIPLIER, SIZE_CAP_FLOOR)


def validate(candidate: Genome, baseline: Genome) -> list[str]:
    """Return a list of validation errors ([] = valid). Caps are derived from
    the baseline so a faithful baseline always validates."""
    errs: list[str] = []
    if not candidate.system_prompt.strip():
        errs.append("system_prompt is empty")
    if len(candidate.system_prompt) > _field_cap(len(baseline.system_prompt)):
        errs.append("system_prompt exceeds size cap")
    for name, desc in candidate.tool_desc.items():
        if name not in baseline.tool_desc:
            errs.append(f"unknown tool '{name}' (not in baseline tool set)")
            continue
        if not desc.strip():
            errs.append(f"tool_desc.{name} is empty (load-bearing descriptions must not be blanked)")
        if len(desc) > _field_cap(len(baseline.tool_desc[name])):
            errs.append(f"tool_desc.{name} exceeds size cap")
    return errs


def diff(a: Genome, b: Genome) -> str:
    """Human-readable summary of fields that differ between a and b."""
    out = []
    if a.system_prompt != b.system_prompt:
        out.append(f"system_prompt: {len(a.system_prompt)} -> {len(b.system_prompt)} chars")
    for name in sorted(set(a.tool_desc) | set(b.tool_desc)):
        av, bv = a.tool_desc.get(name), b.tool_desc.get(name)
        if av != bv:
            out.append(f"tool_desc.{name}: {len(av or '')} -> {len(bv or '')} chars")
    return "\n".join(out) if out else "(no changes)"


if __name__ == "__main__":
    g = extract_current()
    print(f"baseline: system_prompt={len(g.system_prompt)} chars, "
          f"{len(g.tool_desc)} tools: {', '.join(sorted(g.tool_desc))}")
    # Round-trip self-check.
    rt = Genome.from_toml(g.to_toml())
    assert rt == g, "ROUND-TRIP FAILED: baseline does not survive to_toml/from_toml"
    errs = validate(g, g)
    assert not errs, f"baseline failed its own validation: {errs}"
    print("round-trip OK; baseline validates against itself.")
