from evals.verification.calibration import score


def cohort(detected):
    cases, rows = [], {}
    for i in range(200):
        identity = str(i)
        cases.append({"id": identity, "group_id": identity,
                      "label": "supported" if i < 150 else "unsupported",
                      "review_status": "assertion_reviewed", "author_exposed": False,
                      "source_kind": "real_task", "input_hash": "a" * 64,
                      "candidate_hash": "b" * 64, "context": "narrative", "polarity": "positive"})
        rows[identity] = {"rules_verdict": "unknown", "call_status": "ok",
                          "answers": {"contradiction": int(150 <= i < 150 + detected), "unsupported_success": 0},
                          "transport_binding_status": "actual_transport_hash_recorded",
                          "request_hash": "c" * 64, "actual_request_hash": "c" * 64}
    spec = {"confidence": .95, "min_supported": 150, "min_unsupported": 50,
            "max_false_block_rate": .02, "min_detection_rate": .5,
            "min_paired_improvements": 5, "template_version": "jev-single-claim-v2",
            "policy_version": "host-rules-v3.3"}
    return cases, rows, spec


def test_five_improvements_are_not_fifty_percent_detection():
    cases, rows, spec = cohort(5)
    result = score(cases, rows, .9, spec)
    assert result["paired_C_vs_B"]["one_sided_sign_test_p"] < .05
    assert result["gate_reasons"] == ["detection_confidence_bound_below_limit"]
    assert not result["gate_met"]


def test_full_detection_meets_gate_but_no_production_authorization():
    cases, rows, spec = cohort(50)
    result = score(cases, rows, .9, spec)
    assert result["gate_met"]
    assert result["detection"]["confidence_lower_bound"] > .9


def test_duplicate_claim_does_not_increase_group_detection():
    cases, rows, spec = cohort(50)
    cases.append({**cases[150], "id": "extra"})
    rows["extra"] = {**rows["150"], "answers": {"contradiction": 0, "unsupported_success": 0}}
    result = score(cases, rows, .9, spec)
    assert result["detection"]["independent_groups"] == 50
    assert result["detection"]["detected_groups"] == 49
