"""Preregister a distinct resource profile without changing earlier cohorts.

No transport or reservations. Only the coordinator registers real cohorts; the
original ultra candidates are excluded from this profile's future denominator.
"""
import argparse
import json
from pathlib import Path

from . import artifact_cohort
from .artifact_formal_intake import cohort
from .artifact_scoring import sealed
from .calibration import digest, load, require
from .collection_manifest import file_hash
from .paid_budget import durable_new

PROFILE_SCHEMA = "buglecat.verification.main-resource-profile.v1"
PROFILE_ID = "deepseek-flash-high-16384-120-v1"
SETTINGS = {"model": "deepseek-flash", "reasoning_effort": "high", "max_tokens": 16384,
            "timeout_s": 120, "max_retries": 0, "provider_protocol": "openai"}


def validate_profile(path):
    value = sealed(load(path))
    require(value["schema"] == PROFILE_SCHEMA and value["profile_id"] == PROFILE_ID
            and value["main_collection_settings"] == SETTINGS
            and all(type(value["main_collection_settings"][key]) is type(expected)
                    for key, expected in SETTINGS.items()), "frozen main resource profile mismatch")
    previous = cohort(value["previous_artifact_cohort_file"])
    require(file_hash(value["previous_artifact_cohort_file"]) == value["previous_artifact_cohort_sha256"]
            and value["previous_artifact_cohort_sha256"] == "72bb061e677766332230cdaebc9898110d192298b03f66a5b531fb7badb5a0be"
            and previous["artifact_hash"] == value["previous_artifact_cohort_hash"]
            and previous["main_collection_settings"] == {**SETTINGS, "reasoning_effort": "ultra",
                                                         "max_tokens": 8192, "timeout_s": 60},
            "previous resource cohort changed")
    require(file_hash(value["diagnostic_file"]) == value["diagnostic_sha256"]
            and value["diagnostic_sha256"] == "054dc456393a12f2e668500d8bda5405af490278bdc37aa441861a3e2cffcfb4"
            and value["reason"] == "reasoning_exhausted_content_empty", "original resource diagnostic changed")
    diagnostic = load(value["diagnostic_file"])
    failures = diagnostic["failures"]
    require(diagnostic["schema"] == "buglecat.verification.main-failure-diagnostic.v1"
            and len(failures) == 4 and len({row["id"] for row in failures}) == 4,
            "four original resource failures required")
    bindings = [artifact_cohort.binding("previous_artifact_cohort", value["previous_artifact_cohort_file"]),
                artifact_cohort.binding("resource_diagnostic", value["diagnostic_file"])]
    for row in failures:
        receipt_file = Path(row["receipt_file"]).resolve(strict=True)
        receipt = load(receipt_file)
        raw_file = receipt_file.with_name("raw-response.json")
        raw = load(raw_file)
        require(receipt["id"] == row["id"] and receipt["call_status"] == row["status"] == "response_content_invalid"
                and receipt["artifact_cohort_sha256"] == value["previous_artifact_cohort_sha256"]
                and receipt["artifact_cohort_hash"] == value["previous_artifact_cohort_hash"]
                and receipt["response_sha256"] == file_hash(raw_file)
                and raw["choices"][0]["finish_reason"] == row["finish_reason"] == "length"
                and len(raw["choices"][0]["message"].get("content") or "") == row["content_chars"] == 0
                and raw["usage"] == row["usage"]
                and row["usage"]["completion_tokens"] == 8192
                and row["usage"]["completion_tokens_details"]["reasoning_tokens"] == 8192,
                "resource diagnosis differs from original raw transport")
        bindings += [artifact_cohort.binding("resource_failure_receipt", receipt_file),
                     artifact_cohort.binding("resource_failure_raw_response", raw_file)]
    return value, bindings


def preregister(source, catalog_file, authorization, protocol_file, profile_file, cohort_id):
    profile, profile_bindings = validate_profile(profile_file)
    original = artifact_cohort.preregister(source, catalog_file, authorization, protocol_file, cohort_id)
    require(original["formal_registration"] is True and original["fixture_only"] is False,
            "fresh formal source/budget gate not met")
    body = {key: value for key, value in original.items() if key != "artifact_hash"}
    body.update(main_collection_settings=SETTINGS,
                main_resource_profile_file=str(Path(profile_file).resolve()),
                main_resource_profile_sha256=file_hash(profile_file), main_resource_profile_hash=profile["artifact_hash"],
                resource_profile_is_distinct_formal_denominator=True,
                previous_resource_candidates_included=False)
    body["bindings"] += [artifact_cohort.binding("main_resource_profile", profile_file),
                         artifact_cohort.binding("profile_cohort_source", __file__)] + profile_bindings
    examples = Path(__file__).resolve().parents[2] / "rust/crates/ncx-core/examples"
    for role, relative in (("profile_collector_source", "verification_artifact_profile_collect.rs"),
                           ("profile_collection_engine_source", "verification_artifact_profile_collect/engine.rs"),
                           ("profile_source_validator", "verification_artifact_profile_collect/support.rs")):
        body["bindings"].append(artifact_cohort.binding(role, examples / relative))
    require(all(file_hash(row["file"]) == row["sha256"] for row in body["bindings"]),
            "profile registration bound file changed")
    require(validate_profile(profile_file)[0] == profile, "profile changed during registration")
    snapshot, _ = artifact_cohort.ledger_snapshot(authorization)
    require(snapshot == body["ledger_snapshot"], "shared ledger changed during profile registration")
    return {**body, "artifact_hash": digest(body)}


def register(source, catalog_file, authorization, protocol_file, profile_file, cohort_id, output):
    require(not Path(output).exists(), "new profile cohort output required")
    value = preregister(source, catalog_file, authorization, protocol_file, profile_file, cohort_id)
    durable_new(output, value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["validate-profile", "register"])
    parser.add_argument("--profile", type=Path, required=True)
    for name in ("source", "catalog", "authorization", "protocol", "output"):
        parser.add_argument(f"--{name}", type=Path)
    parser.add_argument("--cohort-id")
    args = parser.parse_args()
    if args.action == "validate-profile":
        value, _ = validate_profile(args.profile)
    else:
        require(all((args.source, args.catalog, args.authorization, args.protocol, args.cohort_id, args.output)),
                "source/catalog/authorization/protocol/cohort-id/output required")
        value = register(args.source, args.catalog, args.authorization, args.protocol,
                         args.profile, args.cohort_id, args.output)
    print(json.dumps({"artifact_hash": value["artifact_hash"], "new_paid_calls": 0, "production_authorized": False}))


if __name__ == "__main__":
    main()
