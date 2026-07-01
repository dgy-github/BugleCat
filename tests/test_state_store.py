"""AgentStateStore: atomic persistence and crash-window recovery."""

from __future__ import annotations

from nanocodex.agent.state import AgentState, Fact, TaskNode, VerifyResult
from nanocodex.agent.store import AgentStateStore


def _state() -> AgentState:
    return AgentState(
        goal="ship the feature",
        current_plan=[TaskNode(id="n1", kind="code", title="edit")],
        repo_facts=[Fact(text="uses pytest", source_node="r1")],
    )


def test_save_then_load_roundtrip(tmp_path):
    store = AgentStateStore(tmp_path / ".nanocodex")
    state = _state()
    state.verify_results.append(VerifyResult(node_id="n1", status="pass"))
    store.save(state)

    loaded = store.load()
    assert loaded is not None
    assert loaded.goal == "ship the feature"
    assert loaded.current_plan[0].id == "n1"
    assert loaded.repo_facts[0].text == "uses pytest"
    assert loaded.verify_results[0].passed


def test_load_missing_returns_none(tmp_path):
    store = AgentStateStore(tmp_path / ".nanocodex")
    assert store.load() is None
    assert store.exists() is False


def test_checkpoint_written_atomically_and_loaded(tmp_path):
    store = AgentStateStore(tmp_path / ".nanocodex")
    state = _state()
    ckpt = store.new_checkpoint(state, "n1", "did the edit", files_touched=["a.py"])
    store.write_checkpoint(ckpt)
    state.checkpoints.append(ckpt)
    store.save(state)

    loaded = store.load()
    assert loaded.checkpoints[0].summary == "did the edit"
    assert loaded.checkpoints[0].files_touched == ["a.py"]


def test_recovery_reattaches_orphan_checkpoint(tmp_path):
    """Crash window: checkpoint hit disk but state.json was saved before it.

    Recovery must surface the checkpoint anyway, since the work really happened.
    """
    store = AgentStateStore(tmp_path / ".nanocodex")
    state = _state()
    # Save state.json WITHOUT the checkpoint (simulates the pre-checkpoint save).
    store.save(state)
    # Now the checkpoint lands on disk but we crash before re-saving state.
    ckpt = store.new_checkpoint(state, "n1", "edit finished")
    store.write_checkpoint(ckpt)

    loaded = store.load()
    # state.json had zero checkpoints, but recovery reattaches the orphan.
    assert len(loaded.checkpoints) == 1
    assert loaded.checkpoints[0].node_id == "n1"


def test_no_temp_files_left_behind(tmp_path):
    store = AgentStateStore(tmp_path / ".nanocodex")
    store.save(_state())
    leftovers = list((tmp_path / ".nanocodex").glob("*.tmp"))
    assert leftovers == []
