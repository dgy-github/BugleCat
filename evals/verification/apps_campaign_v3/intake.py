"""Original APPS candidate extraction and independent local label primitives.

No HTTP, budget reservation or probability read. The development collector's
original source/SQLite/transport join must authenticate a response before these
primitives are used. Only exact original fence bytes can be executed or scored.
"""
import json
import os
import re
import sys
from pathlib import Path

from ..apps_solution_v3 import oracle as solution_oracle
from ..apps_v3 import oracle
from ..apps_v3.semantics import CONTRACT, OFFICIAL_COMMIT, OFFICIAL_EVALUATOR_SHA256
from . import packet, resources
from .source_authority import require

ORACLE_SCHEMA = 'buglecat.verification.apps-original-candidate-oracle.v3'
FENCES = re.compile(r'(?ms)^```([^\n]*)\n(.*?)^```[ \t]*$')


def original_code(answer):
    """One Python fence, exact source text and UTF8 span; never repair output."""
    require(type(answer) is str, 'original_main_answer_required')
    matches = list(FENCES.finditer(answer))
    if len(matches) != 1 or matches[0].group(1).strip().lower() not in {'py', 'python'}:
        return None
    match = matches[0]
    code = match.group(2)
    if not code.strip():
        return None
    begin = len(answer[:match.start(2)].encode())
    end = len(answer[:match.end(2)].encode())
    raw = answer.encode()
    require(raw[begin:end].decode() == code, 'original_fence_span_changed')
    return {'code': code, 'code_sha256': oracle.raw_sha(code.encode()),
            'answer_sha256': oracle.raw_sha(raw), 'start': begin, 'end': end}


def execution_owner(kind):
    require(kind in packet.KINDS, 'explicit_APPS_call_mode_required')
    return solution_oracle if kind == packet.SOLUTION else oracle


def runtime():
    return {**packet.runtime_info(), 'version': sys.version}


def checked_oracle(record, task, extracted):
    """Authenticate original task, exact code, true pairs and frozen runtime."""
    plain = {key: item for key, item in record.items() if key != 'artifact_hash'}
    require(record.get('artifact_hash') == oracle.digest(plain)
            and record.get('schema') == ORACLE_SCHEMA, 'original_APPS_oracle_seal_required')
    require(record['task_id'] == task['task_id'] and record['task_sha256'] == oracle.digest(task)
            and record['source_kind'] == task['source_kind'] and record['original_code_span'] == extracted
            and record['oracle_runtime_binding'] == runtime() and record['candidate_labels_independent_of_jev'] is True
            and type(record['paid_calls']) is int and record['paid_calls'] == 0,
            'original_APPS_oracle_identity_changed')
    receipt = record['execution_receipt']
    owner = execution_owner(task['source_kind'])
    expected_schema = ('buglecat.verification.apps-solution-execution-receipt.v3'
                       if task['source_kind'] == packet.SOLUTION else
                       'buglecat.verification.apps-execution-receipt.v3')
    require(receipt['schema'] == expected_schema and receipt['comparison_contract'] == CONTRACT
            and receipt['official_commit'] == OFFICIAL_COMMIT
            and receipt['official_evaluator_sha256'] == OFFICIAL_EVALUATOR_SHA256
            and receipt['source_graph_sha256'] == {role: oracle.raw_sha(path.read_bytes())
                                             for role, path in owner.source_graph().items()}
            and receipt['code_sha256'] == extracted['code_sha256']
            and receipt['input_output_sha256'] == task['input_output_sha256']
            and receipt['pair_count'] == len(task['input_output']['inputs'])
            and receipt['python_version'] == sys.version and receipt['timeout_seconds'] == 5
            and receipt['credentials_removed'] is True and receipt['independent_of_jev'] is True
            and type(receipt['paid_calls']) is int and receipt['paid_calls'] == 0,
            'original_APPS_oracle_execution_changed')
    result = receipt['result']
    require(oracle.checked_result(result, receipt['pair_count']) == result, 'original_APPS_oracle_result_invalid')
    return result


def execute_candidate(task, original_answer):
    """Explicit local execution, once per original candidate; no label guessing."""
    extracted = original_code(original_answer)
    require(extracted is not None, 'one_original_python_fence_required')
    # Resource refusal is Unknown; the caller retains the original main charge.
    resources.checked_request(task, extracted['code'])
    before = runtime()
    receipt = execution_owner(task['source_kind']).execute(extracted['code'], task['input_output'], timeout=5)
    require(runtime() == before, 'actual_runtime_changed_during_candidate_execution')
    body = {'schema': ORACLE_SCHEMA, 'task_id': task['task_id'], 'task_sha256': oracle.digest(task),
            'source_kind': task['source_kind'], 'original_code_span': extracted,
            'oracle_runtime_binding': before, 'execution_receipt': receipt,
            'candidate_labels_independent_of_jev': True, 'paid_calls': 0}
    record = {**body, 'artifact_hash': oracle.digest(body)}
    checked_oracle(record, task, extracted)
    return record


def label(task, original_answer, original_oracle=None):
    """Read-only classification. Missing evidence stays Unknown; no execution."""
    extracted = original_code(original_answer)
    if extracted is None:
        return {'label': 'unknown', 'reason': 'one_original_python_fence_required', 'call_eligible': False}
    try:
        resources.checked_request(task, extracted['code'])
    except ValueError:
        return {'label': 'unknown', 'reason': 'original_complete_packet_resource_refused', 'call_eligible': False,
                'original_code_span': extracted}
    if original_oracle is None:
        return {'label': 'unknown', 'reason': 'independent_original_oracle_not_recorded', 'call_eligible': False,
                'original_code_span': extracted}
    result = checked_oracle(original_oracle, task, extracted)
    binary = {'passed': 'supported', 'failed': 'unsupported'}.get(result['status'], 'unknown')
    return {'label': binary, 'reason': 'original_ordered_pairs_only' if binary != 'unknown' else result['reason'],
            'call_eligible': binary != 'unknown', 'original_code_span': extracted,
            'oracle_hash': original_oracle['artifact_hash'], 'formal_certification': False}


def create_oracle(path, task, original_answer):
    """Reserve a new local receipt filename before executing, never overwrite.

    An interrupted write remains a failed proof and cannot silently rerun. This
    is a local evidence file, not a paid budget reservation.
    """
    with Path(path).open('x', encoding='utf-8') as stream:
        record = execute_candidate(task, original_answer)
        json.dump(record, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    return record
