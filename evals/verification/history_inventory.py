"""Read-only, metadata-only inventory of real tool traces for label preparation.

No user prose, commands, output excerpts, keys, or local source paths are exported.
This inventories execution evidence; it does not label assistant truthfulness.
"""

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path


def inventory(source: Path) -> dict:
    raw = source.read_bytes()
    session_group = hashlib.sha256(raw).hexdigest()
    messages = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    calls = {}
    rows = []
    turn = 0
    for index, message in enumerate(messages):
        if message.get("role") == "user":
            turn += 1
        for call in message.get("tool_calls", []):
            function = call.get("function", {})
            calls[call["id"]] = {"tool": function.get("name"), "turn": turn}
        if message.get("role") != "tool":
            continue
        content = message.get("content")
        if not isinstance(content, str):
            content = json.dumps(content, ensure_ascii=False)
        call = calls.get(message.get("tool_call_id"), {})
        tool = message.get("name", call.get("tool"))
        # Legacy serialized text is an annotation clue, not host certification.
        linked_shell = tool == "shell" and call.get("tool") == "shell" and call.get("turn") == turn
        codes = re.findall(r"(?m)^Exit code:\s*(-?\d+)\s*$", content) if linked_shell else []
        exit_code = int(codes[0]) if len(codes) == 1 else None
        status = "unknown" if exit_code is None else ("passed" if exit_code == 0 else "failed")
        rows.append({"source_index": index, "turn": turn, "tool": tool,
                     "call_reference_present": bool(call),
                     "same_turn_reference": call.get("turn") == turn if call else False,
                     "content_sha256": hashlib.sha256(content.encode()).hexdigest(),
                     "exit_code": exit_code, "execution_label": status,
                     "label_basis": "explicit shell exit contract" if exit_code is not None else "not established",
                     "completion_label": "unlabeled", "file_freshness": "not_available"})
    return {"schema": "buglecat.verification.history-inventory.v1",
            "session_group": session_group, "source_sha256": session_group,
            "message_n": len(messages), "turn_n": turn, "tool_result_n": len(rows),
            "execution_counts": dict(Counter(r["execution_label"] for r in rows)),
            "completion_labels": "not established; independent case review still required",
            "authority": "legacy transcript text only; not current runtime certification",
            "privacy": "only hashes/position/tool/status; raw source stays local",
            "rows": rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = inventory(args.source)
    # Refuse replacing a source or any prior report accidentally.
    if args.output.resolve() == args.source.resolve():
        raise ValueError("output must differ from source")
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps({name: report[name] for name in ("message_n", "turn_n", "tool_result_n", "execution_counts")}))


if __name__ == "__main__":
    main()
