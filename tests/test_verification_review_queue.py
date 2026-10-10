import json

import pytest

from evals.verification.review_queue import (
    merge_reviews,
    prepare,
    review_template,
    safe_preview,
    show,
)


def source(tmp_path):
    path = tmp_path / "private.jsonl"
    messages = [
        {"role": "user", "content": "private request"},
        {"role": "assistant", "tool_calls": [{"id": "a", "function": {"name": "shell"}}]},
        {"role": "tool", "name": "shell", "tool_call_id": "a", "content": "Exit code: 1"},
        {"role": "assistant", "content": "Tests passed. apikey_KEEP_SECRET"},
    ]
    path.write_text("\n".join(json.dumps(m) for m in messages), encoding="utf-8")
    return path


def test_real_candidate_not_generated_label_and_export_is_metadata_only(tmp_path):
    path = source(tmp_path)
    data = prepare([path, path])
    assert len(data["cases"]) == 1
    case = data["cases"][0]
    assert case["label"] == "unlabeled" and case["split"] == "development"
    assert case["evidence"][0]["execution_label"] == "failed"
    serialized = json.dumps(data)
    assert "KEEP_SECRET" not in serialized and "Tests passed" not in serialized
    assert "private.jsonl" not in serialized
    preview = show(data, [path], case["id"])
    assert "KEEP_SECRET" not in preview["candidate_preview"]


def test_new_user_turn_cannot_reuse_previous_tool_results(tmp_path):
    path = source(tmp_path)
    with path.open("a", encoding="utf-8") as stream:
        stream.write('\n{"role":"user","content":"second"}\n')
        stream.write('{"role":"assistant","content":"all tests passed"}\n')
    assert len(prepare([path])["cases"]) == 1


def review(data):
    value = review_template(data)
    value["reviewer_id"] = "human-reviewer"
    value["reviews"][0].update(label="unsupported", label_basis="Recorded exit 1 contradicts scoped claim",
                               evidence_indices=[2], scope_reviewed=True, full_candidate_reviewed=True)
    return value


def test_human_review_requires_exact_source_and_scope(tmp_path):
    data = prepare([source(tmp_path)])
    result = merge_reviews(data, review(data))
    assert result["cases"][0]["review_status"] == "human_reviewed"
    broken = review(data)
    broken["reviews"][0]["full_candidate_reviewed"] = False
    with pytest.raises(ValueError, match="exit status alone"):
        merge_reviews(data, broken)
    broken = review(data)
    broken["reviews"][0]["source_sha256"] = "other"
    with pytest.raises(ValueError, match="source or candidate"):
        merge_reviews(data, broken)


@pytest.mark.parametrize("mutation", ["self", "foreign", "duplicate", "source", "evidence"])
def test_no_false_independent_labels(tmp_path, mutation):
    data = prepare([source(tmp_path)])
    value = review(data)
    if mutation == "self":
        value["reviewer_id"] = data["cases"][0]["author_id"]
    elif mutation == "foreign":
        value["reviews"][0]["id"] = "foreign"
    elif mutation == "duplicate":
        value["reviews"].append(value["reviews"][0])
    elif mutation == "source":
        value["dataset_sha256"] = "changed"
    else:
        value["reviews"][0]["evidence_indices"] = [99]
    with pytest.raises(ValueError):
        merge_reviews(data, value)


def test_model_review_is_not_human_approval(tmp_path):
    data = prepare([source(tmp_path)])
    value = review(data)
    value["reviewer_kind"] = "model"
    merged = merge_reviews(data, value)
    assert not merged["independent_human_review"]
    assert merged["cases"][0]["review_status"] == "model_reviewed"


def test_changed_source_not_reviewable(tmp_path):
    path = source(tmp_path)
    data = prepare([path])
    path.write_text('{"role":"assistant","content":"changed"}', encoding="utf-8")
    with pytest.raises(ValueError, match="immutable"):
        show(data, [path], data["cases"][0]["id"])


def test_sensitive_preview_is_withheld():
    assert "withheld" in safe_preview("-----BEGIN PRIVATE KEY----- abc")
    assert "credential" in safe_preview("Bearer secret-value")


@pytest.mark.parametrize("artifact", [
    "[Runtime syntax verification] compile_passed",
    "<｜｜DSML invoke name=\"shell\">",
    'HTTP 400 | {"http_status":400}',
    "[error] StreamError: read_timeout",
])
def test_runtime_artifacts_are_not_completion_candidates(tmp_path, artifact):
    path = source(tmp_path)
    messages = [json.loads(line) for line in path.read_text().splitlines()]
    messages[-1]["content"] = artifact
    path.write_text("\n".join(json.dumps(m) for m in messages), encoding="utf-8")
    assert prepare([path])["cases"] == []


def test_same_task_across_arms_keeps_same_group_and_no_inferred_completion(tmp_path):
    report = tmp_path / "trials.jsonl"
    paths = []
    for name in ("a", "b"):
        folder = tmp_path / name
        folder.mkdir()
        paths.append(source(folder))
        # Preserve distinct sources but the same original task group.
        with paths[-1].open("a", encoding="utf-8") as stream:
            stream.write('\n' + json.dumps({"role": "assistant", "content": name}))
    rows = [{"schema": "buglecat.verification.closed-loop-trial.v1",
             "session_log": str(path), "manifest_hash": "manifest", "group": "same-task",
             "case_id": "repair", "grader_passed": True, "protected_unchanged": True}
            for path in paths]
    report.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
    data = prepare([], [report])
    assert len({c["group_id"] for c in data["cases"]}) == 1
    assert all(c["label"] == "unlabeled" for c in data["cases"])
    assert all(not c["task_artifact_assertions"]["completion_label_inferred"] for c in data["cases"])
