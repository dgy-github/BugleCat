"""Scoped Agent manifest (``ncx-scope/v1``) loader and runtime narrowing (M0).

Python mirror of ``rust/crates/ncx-core/src/scope.py`` so CLI/train/eval tooling
parses, validates, and enforces the exact same scope TOML contract.
"""

from __future__ import annotations

import os
import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from nanocodex.sandbox.policy import READ_ONLY, WORKSPACE_WRITE, SandboxPolicy

SCOPE_SCHEMA = "ncx-scope/v1"
SCOPE_ENV = "NCX_SCOPE"
VALID_ESCALATION_MODES = ("fallback", "human", "refuse")
_SCOPE_ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,128}$")


class ScopeError(ValueError):
    """Raised when a scope manifest is missing or violates ``ncx-scope/v1``."""


@dataclass(frozen=True)
class ScopeDuty:
    summary: str = ""
    accepts: tuple[str, ...] = ()
    input: str = ""
    output: str = ""


@dataclass(frozen=True)
class ScopeData:
    read_roots: tuple[Path, ...] = ()
    memory_namespace: str | None = None


@dataclass(frozen=True)
class ScopeApprovalRule:
    when: str
    reason: str = ""

    def matches(self, tool_name: str, args: dict[str, Any]) -> bool:
        return _eval_or_expr(self.when.strip(), tool_name, args)


@dataclass(frozen=True)
class ScopeTools:
    allow: tuple[str, ...] = ()
    deny: tuple[str, ...] = ()
    writable_roots: tuple[Path, ...] = ()
    network: bool = False
    os_sandbox: str | None = None
    approval: tuple[ScopeApprovalRule, ...] = ()


@dataclass(frozen=True)
class ScopeModel:
    base: str | None = None
    adapter: str | None = None
    endpoint: str | None = None
    fallback: str | None = None


@dataclass(frozen=True)
class ScopeAcceptance:
    suite: Path | None = None
    pass_rate: float | None = None
    no_regression: bool = True


@dataclass(frozen=True)
class ScopeEscalation:
    on_out_of_scope: str = "fallback"
    on_low_confidence: str = "human"


@dataclass(frozen=True)
class ScopeManifest:
    schema: str
    id: str
    name: str
    version: int
    duty: ScopeDuty = field(default_factory=ScopeDuty)
    data: ScopeData = field(default_factory=ScopeData)
    tools: ScopeTools = field(default_factory=ScopeTools)
    model: ScopeModel = field(default_factory=ScopeModel)
    acceptance: ScopeAcceptance = field(default_factory=ScopeAcceptance)
    escalation: ScopeEscalation = field(default_factory=ScopeEscalation)

    @classmethod
    def from_toml_str(cls, raw: str) -> "ScopeManifest":
        try:
            data = tomllib.loads(raw)
        except tomllib.TOMLDecodeError as exc:
            raise ScopeError(f"invalid scope TOML: {exc}") from exc

        duty_raw = data.get("duty") or {}
        data_raw = data.get("data") or {}
        tools_raw = data.get("tools") or {}
        model_raw = data.get("model") or {}
        acc_raw = data.get("acceptance") or {}
        esc_raw = data.get("escalation") or {}

        approvals = tuple(
            ScopeApprovalRule(
                when=str(item.get("when", "")),
                reason=str(item.get("reason", "")),
            )
            for item in (tools_raw.get("approval") or [])
            if isinstance(item, dict)
        )

        manifest = cls(
            schema=str(data.get("schema", "")),
            id=str(data.get("id", "")),
            name=str(data.get("name", "")),
            version=int(data.get("version", 0)),
            duty=ScopeDuty(
                summary=str(duty_raw.get("summary", "")),
                accepts=tuple(str(x) for x in (duty_raw.get("accepts") or ())),
                input=str(duty_raw.get("input", "")),
                output=str(duty_raw.get("output", "")),
            ),
            data=ScopeData(
                read_roots=tuple(Path(p) for p in (data_raw.get("read_roots") or ())),
                memory_namespace=(
                    str(data_raw["memory_namespace"])
                    if data_raw.get("memory_namespace") is not None
                    else None
                ),
            ),
            tools=ScopeTools(
                allow=tuple(str(x) for x in (tools_raw.get("allow") or ())),
                deny=tuple(str(x) for x in (tools_raw.get("deny") or ())),
                writable_roots=tuple(Path(p) for p in (tools_raw.get("writable_roots") or ())),
                network=bool(tools_raw.get("network", False)),
                os_sandbox=(
                    str(tools_raw["os_sandbox"])
                    if tools_raw.get("os_sandbox") is not None
                    else None
                ),
                approval=approvals,
            ),
            model=ScopeModel(
                base=model_raw.get("base"),
                adapter=model_raw.get("adapter"),
                endpoint=model_raw.get("endpoint"),
                fallback=model_raw.get("fallback"),
            ),
            acceptance=ScopeAcceptance(
                suite=Path(acc_raw["suite"]) if acc_raw.get("suite") else None,
                pass_rate=(
                    float(acc_raw["pass_rate"])
                    if acc_raw.get("pass_rate") is not None
                    else None
                ),
                no_regression=bool(acc_raw.get("no_regression", True)),
            ),
            escalation=ScopeEscalation(
                on_out_of_scope=str(esc_raw.get("on_out_of_scope", "fallback")),
                on_low_confidence=str(esc_raw.get("on_low_confidence", "human")),
            ),
        )
        manifest.validate()
        return manifest

    def validate(self) -> None:
        if self.schema.strip() != SCOPE_SCHEMA:
            raise ScopeError(
                f"unsupported scope schema {self.schema!r}; expected {SCOPE_SCHEMA!r}"
            )
        if not valid_scope_id(self.id):
            raise ScopeError(f"invalid scope id {self.id!r}")
        if not self.name.strip():
            raise ScopeError("scope 'name' must not be empty")
        if self.version < 1:
            raise ScopeError("scope 'version' must be >= 1")
        if self.data.memory_namespace is not None and not valid_scope_id(
            self.data.memory_namespace
        ):
            raise ScopeError(
                f"invalid data.memory_namespace {self.data.memory_namespace!r}"
            )
        if self.tools.os_sandbox is not None and self.tools.os_sandbox.strip() not in (
            "off",
            "enforce",
        ):
            raise ScopeError(f"invalid tools.os_sandbox {self.tools.os_sandbox!r}")
        for rule in self.tools.approval:
            if not rule.when.strip():
                raise ScopeError("tools.approval.when must not be empty")
            _validate_when_expr(rule.when)
        if self.acceptance.pass_rate is not None and not (
            0.0 <= self.acceptance.pass_rate <= 1.0
        ):
            raise ScopeError(f"invalid acceptance.pass_rate {self.acceptance.pass_rate}")
        for label, mode in (
            ("escalation.on_out_of_scope", self.escalation.on_out_of_scope),
            ("escalation.on_low_confidence", self.escalation.on_low_confidence),
        ):
            if mode.strip() not in VALID_ESCALATION_MODES:
                raise ScopeError(
                    f"invalid {label} {mode!r}; expected one of {VALID_ESCALATION_MODES}"
                )

    def matches_accepts(self, query: str) -> bool:
        if not self.duty.accepts:
            return True
        folded = query.lower()
        return any(kw.strip().lower() in folded for kw in self.duty.accepts if kw.strip())


@dataclass(frozen=True)
class LoadedScope:
    manifest: ScopeManifest
    source_path: Path

    @classmethod
    def load_file(cls, path: str | Path) -> "LoadedScope":
        p = Path(path)
        try:
            text = p.read_text(encoding="utf-8")
        except OSError as exc:
            raise ScopeError(f"cannot read scope file {p}: {exc}") from exc
        return cls(manifest=ScopeManifest.from_toml_str(text), source_path=p.resolve())

    def tool_sets(self) -> tuple[set[str] | None, set[str]]:
        allow = set(self.manifest.tools.allow) if self.manifest.tools.allow else None
        deny = set(self.manifest.tools.deny)
        return allow, deny

    def narrow_sandbox(self, base: SandboxPolicy) -> SandboxPolicy:
        """Monotonically narrow *base* without widening global mode or network."""
        allowed_writes: list[Path] = []
        agents_dir = (base.workspace / "agents").resolve()
        if base.writes_allowed:
            for root in self.manifest.tools.writable_roots:
                resolved = (root if root.is_absolute() else base.workspace / root).resolve()
                if resolved == self.source_path or resolved == agents_dir or agents_dir in resolved.parents:
                    continue
                if base.can_write(resolved):
                    allowed_writes.append(resolved)
        if not allowed_writes:
            return SandboxPolicy(
                mode=READ_ONLY,
                workspace=base.workspace,
                writable_roots=[],
                network_access=base.network_access and self.manifest.tools.network,
                allow_temp_write=False,
            )
        # Keep workspace anchored on the first allowed root when a single root is
        # declared, and pass remaining roots as writable_roots so writes outside
        # `tools.writable_roots` are rejected.
        return SandboxPolicy(
            mode=WORKSPACE_WRITE,
            workspace=allowed_writes[0],
            writable_roots=allowed_writes[1:],
            network_access=base.network_access and self.manifest.tools.network,
            allow_temp_write=False,
        )


def valid_scope_id(value: str) -> bool:
    return bool(_SCOPE_ID_RE.match((value or "").strip()))


def resolve_scope(
    workspace: str | Path,
    selector: str,
    *,
    home: str | Path | None = None,
) -> LoadedScope:
    ws = Path(workspace).resolve()
    trimmed = (selector or "").strip()
    if not trimmed:
        raise ScopeError("scope selector cannot be empty")
    if trimmed.endswith(".toml") or "/" in trimmed or "\\" in trimmed:
        cand = Path(trimmed)
        return LoadedScope.load_file(cand if cand.is_absolute() else ws / cand)
    if not valid_scope_id(trimmed):
        raise ScopeError(f"invalid scope id {trimmed!r}")
    ws_cand = ws / "agents" / f"{trimmed}.toml"
    if ws_cand.is_file():
        loaded = LoadedScope.load_file(ws_cand)
        if loaded.manifest.id != trimmed:
            raise ScopeError(
                f"scope file {ws_cand} declares id {loaded.manifest.id!r}, expected {trimmed!r}"
            )
        return loaded
    home_dir = Path(home) if home is not None else Path.home()
    user_cand = home_dir / ".nanocodex" / "agents" / f"{trimmed}.toml"
    if user_cand.is_file():
        loaded = LoadedScope.load_file(user_cand)
        if loaded.manifest.id != trimmed:
            raise ScopeError(
                f"scope file {user_cand} declares id {loaded.manifest.id!r}, expected {trimmed!r}"
            )
        return loaded
    raise ScopeError(f"scope {trimmed!r} not found")


def resolve_scope_from_env(workspace: str | Path) -> LoadedScope | None:
    raw = os.environ.get(SCOPE_ENV, "").strip()
    return resolve_scope(workspace, raw) if raw else None


# ── Conditional approval expression evaluator ─────────────────────────────────


def _validate_when_expr(expr: str) -> None:
    trimmed = expr.strip()
    if trimmed in ("*", "always", "true"):
        return
    for or_part in trimmed.split("||"):
        for and_part in or_part.split("&&"):
            clause = and_part.strip()
            if clause in ("*", "always", "true"):
                continue
            if _parse_clause(clause) is None:
                raise ScopeError(f"unsupported tools.approval.when clause {clause!r}")


def _parse_clause(clause: str) -> tuple[str, str, str] | None:
    for op in (">=", "<=", "!=", "==", ">", "<", " contains "):
        if op in clause:
            lhs, rhs = clause.split(op, 1)
            if lhs.strip() and rhs.strip():
                return lhs.strip(), op.strip(), rhs.strip().strip("\"'")
    return None


def _eval_or_expr(expr: str, tool_name: str, args: dict[str, Any]) -> bool:
    if expr in ("*", "always", "true"):
        return True
    return any(
        all(_eval_clause(c.strip(), tool_name, args) for c in or_part.split("&&"))
        for or_part in expr.split("||")
    )


def _eval_clause(clause: str, tool_name: str, args: dict[str, Any]) -> bool:
    if clause in ("*", "always", "true"):
        return True
    parsed = _parse_clause(clause)
    if parsed is None:
        return False
    field_name, op, rhs = parsed
    values = [tool_name] if field_name == "tool" else _collect_values(field_name, args)
    return any(_compare(val, op, rhs) for val in values)


def _collect_values(field_name: str, obj: Any) -> list[Any]:
    out: list[Any] = []
    if isinstance(obj, dict):
        if field_name in obj:
            out.append(obj[field_name])
        for val in obj.values():
            out.extend(_collect_values(field_name, val))
    elif isinstance(obj, list):
        for item in obj:
            out.extend(_collect_values(field_name, item))
    elif isinstance(obj, str):
        pattern = re.compile(
            rf'(?:["\']{re.escape(field_name)}["\']\s*:\s*|{re.escape(field_name)}=|--{re.escape(field_name)}\s+)([^\s,}}\];"\']+)'
        )
        for match in pattern.finditer(obj):
            out.append(match.group(1))
    return out


def _compare(lhs: Any, op: str, rhs: str) -> bool:
    try:
        l_num = float(lhs)
        r_num = float(rhs)
        if op == ">":
            return l_num > r_num
        if op == ">=":
            return l_num >= r_num
        if op == "<":
            return l_num < r_num
        if op == "<=":
            return l_num <= r_num
        if op == "==":
            return l_num == r_num
        if op == "!=":
            return l_num != r_num
    except (TypeError, ValueError):
        pass
    l_str = str(lhs)
    if op == "==":
        return l_str == rhs
    if op == "!=":
        return l_str != rhs
    if op == "contains":
        return rhs in l_str
    return False
