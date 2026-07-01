"""Configuration loading for nanocodex.

Resolution order (highest priority wins):

    explicit overrides (CLI)  >  environment  >  ~/.nanocodex/config.toml
                              >  ~/.deepseek/config.toml  >  ~/.codex/config.toml
                              >  built-in defaults

``~/.nanocodex/config.toml`` is nanocodex's own settings file (written by the
GUI Settings dialog). It sits above the DeepSeek/Codex files so a value the user
sets in the GUI wins over the CLIs' own configs, but still below environment and
explicit CLI flags.

The API key is read at runtime from the config file or environment and is
never logged, printed, or returned by :meth:`Config.redacted`.
"""

from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

DEEPSEEK_CONFIG = Path.home() / ".deepseek" / "config.toml"
CODEX_CONFIG = Path.home() / ".codex" / "config.toml"
# nanocodex's own, isolated settings file. Written by the GUI Settings dialog.
# Deliberately separate from ~/.deepseek/config.toml (owned by DeepSeek-CLI) so
# editing settings here never rewrites or reorders that file's content.
NANOCODEX_CONFIG = Path.home() / ".nanocodex" / "config.toml"

# Sensible fallbacks if nothing else is configured.
_DEFAULT_BASE_URL = "https://api.deepseek.com/v1"
_DEFAULT_MODEL = "deepseek-chat"

VALID_SANDBOX_MODES = ("read-only", "workspace-write", "danger-full-access")
VALID_APPROVAL_POLICIES = ("untrusted", "on-failure", "on-request", "never")


class ConfigError(RuntimeError):
    """Raised when configuration is missing or invalid."""


@dataclass
class Config:
    """Resolved runtime configuration."""

    api_key: str
    base_url: str
    model: str
    sandbox_mode: str = "workspace-write"
    approval_policy: str = "on-request"
    reasoning_effort: str = "auto"
    # Vision (VL) endpoint for image-bearing turns. Empty = no separate vision
    # backend (image turns then go to the main model, which may be text-only).
    # Lets nanocodex route a turn that carries an image to a vision-capable model
    # (e.g. DashScope Qwen-VL) while keeping DeepSeek for text/coding. base_url/
    # api_key fall back to the main ones when only vl_model is set (same vendor).
    vl_base_url: str = ""
    vl_api_key: str = ""
    vl_model: str = ""
    # Volcengine ARK key for Seedance video rendering (storyboard 出片). Only
    # needed when actually rendering; planning/preview never touches it.
    ark_api_key: str = ""
    workspace: Path = field(default_factory=Path.cwd)
    writable_roots: list[Path] = field(default_factory=list)
    network_access: bool = False
    max_iterations: int = 60
    timeout_s: int = 120
    # How many times the OpenAI-compatible SDK retries a transient model call
    # (connection error / 408 / 409 / 429 / >=500) before surfacing the failure,
    # with exponential backoff that honors Retry-After. The SDK default is 2; we
    # bump it to 3 and make it tunable (NANOCODEX_MAX_RETRIES) so a flaky network
    # / rate-limited endpoint recovers without killing the whole turn. 4xx other
    # than 408/409/429 (auth, bad-request) are NOT retried — those are permanent.
    max_retries: int = 3
    # Approximate prompt token budget that triggers context compaction.
    # 0 disables compaction. Default ON at ~50% of the 1M window (512K): long
    # conversations fold their middle automatically (Codex-style) so the prompt
    # never balloons unbounded and every turn's input cost stays bounded. Set 0
    # (NANOCODEX_CONTEXT_BUDGET / --context-budget) to turn compaction OFF.
    context_token_budget: int = 512_000
    # Model context-window size (tokens), used to display "used / limit" in the
    # GUI and as the reference for the compaction budget. 1M is deepseek-v4-pro's
    # OFFICIAL context length (verified at api-docs.deepseek.com: every current
    # model lists 1M context / 384K max output). Override via
    # NANOCODEX_CONTEXT_WINDOW if a future model differs.
    context_window: int = 1_048_576
    # Models offered in the GUI's model-switcher menu. Not written in stone:
    # overridable via NANOCODEX_MODELS (comma-separated). The active `model`
    # is always merged in so it's selectable even if not listed here.
    available_models: list[str] = field(default_factory=lambda: [
        "deepseek-v4-pro", "deepseek-chat", "deepseek-reasoner",
    ])

    def validate(self) -> None:
        if not self.api_key:
            raise ConfigError(
                "No API key found. Set DEEPSEEK_API_KEY or add api_key to "
                f"{DEEPSEEK_CONFIG}."
            )
        if self.sandbox_mode not in VALID_SANDBOX_MODES:
            raise ConfigError(
                f"Invalid sandbox_mode {self.sandbox_mode!r}; "
                f"expected one of {VALID_SANDBOX_MODES}."
            )
        if self.approval_policy not in VALID_APPROVAL_POLICIES:
            raise ConfigError(
                f"Invalid approval_policy {self.approval_policy!r}; "
                f"expected one of {VALID_APPROVAL_POLICIES}."
            )

    def redacted(self) -> dict[str, Any]:
        """Config snapshot safe to display: the API key is masked."""
        masked = "(unset)"
        if self.api_key:
            tail = self.api_key[-4:] if len(self.api_key) >= 4 else ""
            masked = f"****{tail}"
        vl_masked = "(unset)"
        if self.vl_api_key:
            vtail = self.vl_api_key[-4:] if len(self.vl_api_key) >= 4 else ""
            vl_masked = f"****{vtail}"
        ark_masked = "(unset)"
        if self.ark_api_key:
            atail = self.ark_api_key[-4:] if len(self.ark_api_key) >= 4 else ""
            ark_masked = f"****{atail}"
        return {
            "api_key": masked,
            "base_url": self.base_url,
            "model": self.model,
            "sandbox_mode": self.sandbox_mode,
            "approval_policy": self.approval_policy,
            "reasoning_effort": self.reasoning_effort,
            "vl_base_url": self.vl_base_url,
            "vl_api_key": vl_masked,
            "vl_model": self.vl_model,
            "ark_api_key": ark_masked,
            "workspace": str(self.workspace),
            "writable_roots": [str(p) for p in self.writable_roots],
            "network_access": self.network_access,
            "max_iterations": self.max_iterations,
            "timeout_s": self.timeout_s,
            "max_retries": self.max_retries,
        }


def _load_toml(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {}
    try:
        with path.open("rb") as fh:
            return tomllib.load(fh)
    except (OSError, tomllib.TOMLDecodeError):
        return {}


def _deepseek_values(raw: dict[str, Any]) -> dict[str, Any]:
    """Extract the fields we care about from a ~/.deepseek/config.toml dump."""
    out: dict[str, Any] = {}
    if not raw:
        return out
    if "base_url" in raw:
        out["base_url"] = raw["base_url"]
    # default_text_model is DeepSeek-CLI's field name for the chat model.
    if raw.get("default_text_model"):
        out["model"] = raw["default_text_model"]
    elif raw.get("model"):
        out["model"] = raw["model"]
    for key in ("sandbox_mode", "approval_policy", "reasoning_effort"):
        if raw.get(key):
            out[key] = raw[key]
    # API key: prefer top-level, fall back to providers.deepseek.api_key.
    api_key = raw.get("api_key")
    if not api_key:
        providers = raw.get("providers")
        if isinstance(providers, dict):
            ds = providers.get("deepseek")
            if isinstance(ds, dict):
                api_key = ds.get("api_key")
    if api_key:
        out["api_key"] = api_key
    return out


def _nanocodex_values(raw: dict[str, Any]) -> dict[str, Any]:
    """Extract settings from nanocodex's own ~/.nanocodex/config.toml.

    This is a flat file nanocodex fully owns (written by the GUI Settings
    dialog), so the keys match Config field names directly — no aliasing like
    DeepSeek-CLI's ``default_text_model``. Only known scalar keys are read;
    anything else in the file is ignored.
    """
    out: dict[str, Any] = {}
    if not raw:
        return out
    for key in ("api_key", "base_url", "model", "sandbox_mode",
                "approval_policy", "reasoning_effort",
                "vl_base_url", "vl_api_key", "vl_model", "ark_api_key"):
        val = raw.get(key)
        if val:
            out[key] = val
    return out


def _codex_values(raw: dict[str, Any]) -> dict[str, Any]:
    """Extract Codex-style settings from ~/.codex/config.toml."""
    out: dict[str, Any] = {}
    if not raw:
        return out
    if raw.get("model"):
        out["model"] = raw["model"]
    if raw.get("approval_policy"):
        out["approval_policy"] = raw["approval_policy"]
    # Codex stores sandbox under [windows]/[sandbox] in some builds; best-effort.
    if raw.get("sandbox_mode"):
        out["sandbox_mode"] = raw["sandbox_mode"]
    if raw.get("model_reasoning_effort"):
        out["reasoning_effort"] = raw["model_reasoning_effort"]
    return out


# Keys a [profiles.<name>] table may set (a named bundle of model + sandbox +
# approval + reasoning, à la Codex). Restricted to scalar Config fields so a
# profile can't smuggle in arbitrary keys.
_PROFILE_KEYS = (
    "model", "base_url", "sandbox_mode", "approval_policy", "reasoning_effort",
    "vl_base_url", "vl_api_key", "vl_model", "ark_api_key",
    "max_iterations", "max_retries", "context_token_budget", "context_window",
)


def _profile_values(selected: dict[str, Any]) -> dict[str, Any]:
    """Pull the known profile-able keys out of one [profiles.<name>] table."""
    return {k: selected[k] for k in _PROFILE_KEYS if selected.get(k) is not None}


def list_profiles() -> list[str]:
    """Names of [profiles.<name>] tables defined in ~/.nanocodex/config.toml."""
    profiles = _load_toml(NANOCODEX_CONFIG).get("profiles")
    return sorted(profiles) if isinstance(profiles, dict) else []


def load_config(
    *,
    workspace: Path | None = None,
    overrides: dict[str, Any] | None = None,
    profile: str | None = None,
) -> Config:
    """Resolve a :class:`Config` from files, environment, and explicit overrides.

    *overrides* (typically from CLI flags) win over everything else. ``None``
    values inside *overrides* are ignored so callers can pass partial dicts.
    *profile* selects a ``[profiles.<name>]`` bundle from ~/.nanocodex/config.toml
    whose values override the base config files but stay below environment and
    CLI flags. The profile name may also come from ``NANOCODEX_PROFILE`` or a
    top-level ``profile = "…"`` key in that file. An unknown name raises
    :class:`ConfigError`.
    """
    merged: dict[str, Any] = {
        "base_url": _DEFAULT_BASE_URL,
        "model": _DEFAULT_MODEL,
    }

    # Lowest priority: Codex config, then DeepSeek config, then nanocodex's
    # own file (so a value set in the GUI Settings dialog wins over the CLIs').
    nano_raw = _load_toml(NANOCODEX_CONFIG)
    merged.update(_codex_values(_load_toml(CODEX_CONFIG)))
    merged.update(_deepseek_values(_load_toml(DEEPSEEK_CONFIG)))
    merged.update(_nanocodex_values(nano_raw))

    # Profile: a named bundle layered ABOVE the base files, BELOW env/CLI.
    prof_name = profile or os.environ.get("NANOCODEX_PROFILE") or nano_raw.get("profile")
    if prof_name:
        profiles = nano_raw.get("profiles")
        selected = profiles.get(prof_name) if isinstance(profiles, dict) else None
        if not isinstance(selected, dict):
            available = ", ".join(sorted(profiles)) if isinstance(profiles, dict) else "(none)"
            raise ConfigError(
                f"Profile {prof_name!r} not found in {NANOCODEX_CONFIG}. "
                f"Available profiles: {available}."
            )
        merged.update(_profile_values(selected))

    # Environment overrides.
    env_map = {
        "api_key": ("DEEPSEEK_API_KEY", "NANOCODEX_API_KEY"),
        "base_url": ("DEEPSEEK_BASE_URL", "NANOCODEX_BASE_URL"),
        "model": ("NANOCODEX_MODEL",),
        # Vision backend for image-bearing turns (e.g. DashScope Qwen-VL).
        "vl_base_url": ("NANOCODEX_VL_BASE_URL",),
        "vl_api_key": ("DASHSCOPE_API_KEY", "NANOCODEX_VL_API_KEY"),
        "vl_model": ("NANOCODEX_VL_MODEL",),
        # Volcengine ARK key for Seedance video rendering (storyboard 出片).
        "ark_api_key": ("ARK_API_KEY", "NANOCODEX_ARK_API_KEY"),
        "sandbox_mode": ("NANOCODEX_SANDBOX",),
        "approval_policy": ("NANOCODEX_APPROVAL",),
        "context_token_budget": ("NANOCODEX_CONTEXT_BUDGET",),
        "context_window": ("NANOCODEX_CONTEXT_WINDOW",),
        "available_models": ("NANOCODEX_MODELS",),
        "max_iterations": ("NANOCODEX_MAX_ITERATIONS",),
        "max_retries": ("NANOCODEX_MAX_RETRIES",),
    }
    for field_name, env_keys in env_map.items():
        for env_key in env_keys:
            val = os.environ.get(env_key)
            if val:
                merged[field_name] = val
                break

    # Highest priority: explicit overrides.
    if overrides:
        for key, val in overrides.items():
            if val is not None:
                merged[key] = val

    active_model = merged.get("model", _DEFAULT_MODEL)
    cfg = Config(
        api_key=merged.get("api_key", ""),
        base_url=merged.get("base_url", _DEFAULT_BASE_URL),
        model=active_model,
        sandbox_mode=merged.get("sandbox_mode", "workspace-write"),
        approval_policy=merged.get("approval_policy", "on-request"),
        reasoning_effort=merged.get("reasoning_effort", "auto"),
        vl_base_url=merged.get("vl_base_url", ""),
        vl_api_key=merged.get("vl_api_key", ""),
        vl_model=merged.get("vl_model", ""),
        ark_api_key=merged.get("ark_api_key", ""),
        workspace=(workspace or Path.cwd()).resolve(),
        context_token_budget=_as_int(merged.get("context_token_budget"), 512_000),
        context_window=_as_int(merged.get("context_window"), 1_048_576),
        max_iterations=_as_int(merged.get("max_iterations"), 60),
        max_retries=_as_int(merged.get("max_retries"), 3),
        available_models=_model_list(merged.get("available_models"), active_model),
    )
    if cfg.sandbox_mode == "danger-full-access":
        cfg.network_access = True
    return cfg


_DEFAULT_MODELS = ["deepseek-v4-pro", "deepseek-chat", "deepseek-reasoner"]


def _model_list(value: Any, active: str) -> list[str]:
    """Build the model-switcher list from config/env, with the active model first.

    Accepts a comma-separated string (from env) or a list. When nothing is
    configured, falls back to a built-in default set so the switcher always
    offers real choices. The active model is always included and placed first.
    """
    if isinstance(value, str):
        names = [s.strip() for s in value.split(",") if s.strip()]
    elif isinstance(value, (list, tuple)):
        names = [str(s).strip() for s in value if str(s).strip()]
    else:
        names = []
    if not names:
        names = list(_DEFAULT_MODELS)  # nothing configured -> sensible defaults
    ordered = [active] + [n for n in names if n != active]
    # De-duplicate while preserving order.
    seen: set[str] = set()
    out: list[str] = []
    for n in ordered:
        if n and n not in seen:
            seen.add(n)
            out.append(n)
    return out


def _as_int(value: Any, default: int) -> int:
    """Coerce a config value (possibly a string from env) to int."""
    if value is None:
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


# --- writing nanocodex's own config file ------------------------------------
#
# tomllib (stdlib) only *reads* TOML, and nanocodex avoids heavy deps, so we
# ship a tiny purpose-built writer for the flat scalar shape this file uses
# (key = "value"), mirroring tools/mcp_store.py. The file is plain TOML the
# user can also edit by hand.

# Keys the GUI Settings dialog may write. Order here is the order on disk.
_WRITABLE_KEYS = (
    "api_key", "base_url", "model",
    "sandbox_mode", "approval_policy", "reasoning_effort",
    "vl_base_url", "vl_api_key", "vl_model",
    "ark_api_key",
)


def _esc_toml(value: str) -> str:
    """Escape a string for a TOML basic (double-quoted) string."""
    return (
        str(value)
        .replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )


def dump_nanocodex_toml(values: dict[str, Any]) -> str:
    """Serialize known settings into ~/.nanocodex/config.toml text (pure).

    Only keys in :data:`_WRITABLE_KEYS` are emitted, in a fixed order, so the
    output round-trips through :func:`tomllib.loads`. Empty/None values are
    skipped (an unset API key shouldn't be written as an empty string).
    """
    header = (
        "# nanocodex settings. Managed by the GUI Settings dialog, but also\n"
        "# safe to edit by hand. These values win over ~/.deepseek/config.toml\n"
        "# and ~/.codex/config.toml, but environment variables and CLI flags\n"
        "# still override them. The API key is never logged or printed.\n"
    )
    lines: list[str] = []
    for key in _WRITABLE_KEYS:
        val = values.get(key)
        if val:
            lines.append(f'{key} = "{_esc_toml(val)}"')
    return header + "\n" + "\n".join(lines) + ("\n" if lines else "")


def write_nanocodex_config(
    updates: dict[str, Any],
    *,
    path: Path | None = None,
) -> Path:
    """Merge *updates* into ~/.nanocodex/config.toml and write it back.

    Existing values in the file are preserved (this is a merge, not a replace),
    so setting just the API key won't wipe a previously saved base_url/model.
    Only keys in :data:`_WRITABLE_KEYS` are persisted; others are ignored.
    Returns the path written. Raises OSError on write failure.
    """
    target = Path(path) if path is not None else NANOCODEX_CONFIG
    current = _load_toml(target)
    merged = {k: current[k] for k in _WRITABLE_KEYS if current.get(k)}
    for key, val in updates.items():
        if key not in _WRITABLE_KEYS:
            continue
        if val is None:
            continue
        merged[key] = val
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(dump_nanocodex_toml(merged), encoding="utf-8")
    return target
