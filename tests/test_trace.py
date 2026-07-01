"""NCX_TRACE: dimension-filtered tracing + verifier re-ask count plumbing."""

from __future__ import annotations

import pytest

from nanocodex.agent.trace import trace, trace_enabled


def test_disabled_when_env_unset(monkeypatch):
    monkeypatch.delenv("NCX_TRACE", raising=False)
    assert trace_enabled("verifier") is False


@pytest.mark.parametrize("val", ["1", "all", "*", "true", "on", "yes", "ALL"])
def test_global_on_values(monkeypatch, val):
    monkeypatch.setenv("NCX_TRACE", val)
    assert trace_enabled("verifier") is True
    assert trace_enabled("orchestrator") is True


def test_single_dimension_allowlist(monkeypatch):
    monkeypatch.setenv("NCX_TRACE", "verifier")
    assert trace_enabled("verifier") is True
    assert trace_enabled("orchestrator") is False


def test_comma_separated_dimensions(monkeypatch):
    monkeypatch.setenv("NCX_TRACE", "verifier, orchestrator")
    assert trace_enabled("verifier") is True
    assert trace_enabled("orchestrator") is True
    assert trace_enabled("planner") is False


def test_trace_writes_to_stderr_when_enabled(monkeypatch, capsys):
    monkeypatch.setenv("NCX_TRACE", "verifier")
    trace("verifier", "hello")
    trace("planner", "should not appear")
    err = capsys.readouterr().err
    assert "[trace:verifier] hello" in err
    assert "should not appear" not in err
