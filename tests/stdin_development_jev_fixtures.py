"""Synthetic byte writer only; not available to actual sender/live authority."""
import json
from pathlib import Path

from evals.verification.apps_stdin_v1 import budget, cohort
from evals.verification.apps_stdin_v1 import development_observations as observer


def finish_synthetic(row, prepared_file, reservation_file, *, status='ok'):
    directory = Path(row['output_directory'])
    reserved = budget.load(reservation_file)
    original = observer.identity(row, prepared_file, reserved, reservation_file)
    request = directory / 'request.json'
    request.write_bytes(row['request_json'].encode())
    raw_file = directory / 'raw-response.json'
    response = {'model': 'jev-1.13.0', 'answers': {'acceptance_violation': {'type': 'noul', 'noul': .1}},
                'usage': {'input_tokens': 12, 'output_tokens': 3}}
    raw_file.write_bytes(json.dumps(response).encode() if status == 'ok' else b'{"error":"fixture-only"}')
    terminal = {**original, 'call_status': status, 'http_status': 200 if status == 'ok' else int(status[5:]),
                'raw_response_file': str(raw_file), 'response_sha256': cohort.sha(raw_file),
                'response': response if status == 'ok' else None}
    (directory / 'receipt.json').write_bytes(observer.packet.canonical_bytes(terminal))
    events = [{**original, 'event': 'attempt_started'},
              {**original, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']},
              {**terminal, 'event': 'attempt_finished', 'receipt_sha256': cohort.sha(directory / 'receipt.json')}]
    (directory / 'events.jsonl').write_bytes(b'\n'.join(observer.packet.canonical_bytes(event) for event in events) + b'\n')
    return directory
