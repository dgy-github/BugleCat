#!/usr/bin/env python3
"""Genome model for ncx-forge (M0b + P1 RRSI + P2 expanded surfaces).

A genome is the evolvable harness surface: the base system_prompt, per-tool
descriptions, orchestrator node system prompts, and bounded memory recall caps.
The baseline is extracted from the agent itself via `ncx --dump-genome` (never
by parsing Rust source), so it always reflects the real tool list and the real,
load-bearing descriptions.

Pure stdlib: tomllib (read) + a tiny matching writer (no tomli_w dependency,
no dependency on the legacy nanocodex python package).
"""
from __future__ import annotations

import difflib
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
    orchestrator_prompts: dict[str, str] = field(default_factory=dict)
    memory_config: dict[str, int] = field(default_factory=dict)

    def to_toml(self) -> str:
        lines = [f'system_prompt = "{_toml_escape(self.system_prompt)}"', "", "[tool_desc]"]
        for name in sorted(self.tool_desc):
            lines.append(f'{name} = "{_toml_escape(self.tool_desc[name])}"')
        if self.orchestrator_prompts:
            lines.extend(["", "[orchestrator_prompts]"])
            for k in sorted(self.orchestrator_prompts):
                lines.append(f'{k} = "{_toml_escape(self.orchestrator_prompts[k])}"')
        if self.memory_config:
            lines.extend(["", "[memory_config]"])
            for k in sorted(self.memory_config):
                lines.append(f"{k} = {int(self.memory_config[k])}")
        return "\n".join(lines) + "\n"

    @classmethod
    def from_toml(cls, text: str) -> "Genome":
        d = tomllib.loads(text)
        return cls(
            system_prompt=str(d.get("system_prompt", "")),
            tool_desc={k: str(v) for k, v in (d.get("tool_desc") or {}).items()},
            orchestrator_prompts={
                k: str(v) for k, v in (d.get("orchestrator_prompts") or {}).items()
            },
            memory_config={
                k: int(v)
                for k, v in (d.get("memory_config") or {}).items()
                if isinstance(v, int) and not isinstance(v, bool)
            },
        )

    def save(self, path: Path) -> None:
        path.write_text(self.to_toml(), encoding="utf-8")

    @classmethod
    def load(cls, path: Path) -> "Genome":
        return cls.from_toml(Path(path).read_text(encoding="utf-8"))

    def copy(self) -> "Genome":
        return Genome(
            self.system_prompt,
            dict(self.tool_desc),
            dict(self.orchestrator_prompts),
            dict(self.memory_config),
        )


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


def changed_surfaces(a: Genome, b: Genome) -> list[str]:
    """Return sorted list of surface keys ('system_prompt', 'tool_desc.<name>',
    'orchestrator_prompts.<key>', 'memory_config.<key>') that differ."""
    out: list[str] = []
    if a.system_prompt != b.system_prompt:
        out.append("system_prompt")
    for name in sorted(set(a.tool_desc) | set(b.tool_desc)):
        if a.tool_desc.get(name) != b.tool_desc.get(name):
            out.append(f"tool_desc.{name}")
    for key in sorted(set(a.orchestrator_prompts) | set(b.orchestrator_prompts)):
        if a.orchestrator_prompts.get(key) != b.orchestrator_prompts.get(key):
            out.append(f"orchestrator_prompts.{key}")
    for key in sorted(set(a.memory_config) | set(b.memory_config)):
        if a.memory_config.get(key) != b.memory_config.get(key):
            out.append(f"memory_config.{key}")
    return out


def changed_lines(a: Genome, b: Genome) -> int:
    """Count total added/removed lines across all modified surfaces between a and b."""
    total = 0
    pairs: list[tuple[str, str]] = []
    if a.system_prompt != b.system_prompt:
        pairs.append((a.system_prompt, b.system_prompt))
    for name in sorted(set(a.tool_desc) | set(b.tool_desc)):
        av, bv = a.tool_desc.get(name, ""), b.tool_desc.get(name, "")
        if av != bv:
            pairs.append((av, bv))
    for key in sorted(set(a.orchestrator_prompts) | set(b.orchestrator_prompts)):
        av, bv = a.orchestrator_prompts.get(key, ""), b.orchestrator_prompts.get(key, "")
        if av != bv:
            pairs.append((av, bv))
    for key in sorted(set(a.memory_config) | set(b.memory_config)):
        av, bv = str(a.memory_config.get(key, "")), str(b.memory_config.get(key, ""))
        if av != bv:
            pairs.append((av, bv))
    for old_text, new_text in pairs:
        for line in difflib.unified_diff(
            old_text.splitlines(), new_text.splitlines(), lineterm=""
        ):
            if (line.startswith("+") and not line.startswith("+++")) or (
                line.startswith("-") and not line.startswith("---")
            ):
                total += 1
    return total


def validate(
    candidate: Genome,
    baseline: Genome,
    parent: Genome | None = None,
    max_surfaces: int | None = None,
    max_changed_lines: int | None = None,
) -> list[str]:
    """Return a list of validation errors ([] = valid). Caps are derived from
    the baseline so a faithful baseline always validates. When `parent` and
    annealed edit budgets (`max_surfaces`, `max_changed_lines`) are supplied,
    oversized mutations are rejected before evaluation (RRSI regularization)."""
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
    for key, prompt in candidate.orchestrator_prompts.items():
        if baseline.orchestrator_prompts and key not in baseline.orchestrator_prompts:
            errs.append(f"unknown orchestrator_prompts.{key}")
            continue
        if not prompt.strip():
            errs.append(f"orchestrator_prompts.{key} is empty")
        base_len = len(baseline.orchestrator_prompts.get(key, ""))
        if len(prompt) > _field_cap(base_len):
            errs.append(f"orchestrator_prompts.{key} exceeds size cap")
    for key, val in candidate.memory_config.items():
        if key not in ("max_entries", "max_chars"):
            errs.append(f"unknown memory_config.{key}")
        elif key == "max_entries" and not (1 <= val <= 32):
            errs.append(f"memory_config.max_entries={val} out of bounds [1, 32]")
        elif key == "max_chars" and not (200 <= val <= 16_000):
            errs.append(f"memory_config.max_chars={val} out of bounds [200, 16000]")
    if parent is not None:
        surfaces = changed_surfaces(parent, candidate)
        if max_surfaces is not None and len(surfaces) > max_surfaces:
            errs.append(
                f"exceeds annealed surface budget ({len(surfaces)} > {max_surfaces}: {', '.join(surfaces)})"
            )
        if max_changed_lines is not None:
            n_lines = changed_lines(parent, candidate)
            if n_lines > max_changed_lines:
                errs.append(
                    f"exceeds annealed line budget ({n_lines} > {max_changed_lines})"
                )
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
    for key in sorted(set(a.orchestrator_prompts) | set(b.orchestrator_prompts)):
        av, bv = a.orchestrator_prompts.get(key), b.orchestrator_prompts.get(key)
        if av != bv:
            out.append(f"orchestrator_prompts.{key}: {len(av or '')} -> {len(bv or '')} chars")
    for key in sorted(set(a.memory_config) | set(b.memory_config)):
        av, bv = a.memory_config.get(key), b.memory_config.get(key)
        if av != bv:
            out.append(f"memory_config.{key}: {av} -> {bv}")
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
