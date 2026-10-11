"""Pinned original STDIN rows, AST/text only; no execution or fee authority."""
import json

from ..apps_v3 import source as original
from . import contract, packet

SCHEMA = 'buglecat.verification.apps-stdin-original-task.v1'
SOURCE_KIND = 'apps_stdin_restricted_script'
verified_rows = original.verified_rows


def parse_row(raw_line, line_number):
    """Keep reference zero and full original IO; row identity is not dedup identity."""
    packet.require(type(raw_line) is bytes and type(line_number) is int and line_number > 0,
                   'original row bytes and positive line number required')
    row = json.loads(raw_line, parse_constant=original.reject_nonfinite)
    packet.require(type(row) is dict and type(row.get('id')) is int, 'invalid original task id')
    packet.require(type(row.get('question')) is str and bool(row['question'].strip()),
                   'complete original question required')
    packet.require(type(row.get('starter_code')) is str, 'complete original starter required')
    packet.require(row.get('url') is None or type(row['url']) is str, 'original URL shape invalid')
    io_raw, references_raw = row.get('input_output'), row.get('solutions')
    packet.require(type(io_raw) is str and type(references_raw) is str, 'original JSON fields required')
    io = json.loads(io_raw, parse_constant=original.reject_nonfinite)
    references = json.loads(references_raw, parse_constant=original.reject_nonfinite)
    packet.require(type(io) is dict and set(io) in ({'inputs', 'outputs'}, {'inputs', 'outputs', 'fn_name'}),
                   'original STDIN IO fields required')
    contract.ordered_pairs(io)
    packet.require(type(references) is list and bool(references) and
                   type(references[0]) is str and bool(references[0].strip()), 'original reference zero required')
    # Never search a later reference when the first lies outside the contract.
    contract.reviewed_tree(references[0])
    return {'schema': SCHEMA, 'source_kind': SOURCE_KIND, 'original_split': 'train',
            'revision': original.REVISION, 'full_source_sha256': original.TRAIN_SHA256,
            'task_id': row['id'], 'group_id': f"apps-{original.REVISION}-train-{row['id']}",
            'original_line_number': line_number, 'original_line_sha256': original.raw_sha(raw_line),
            'question': row['question'], 'starter_code': row['starter_code'], 'url': row.get('url'),
            'input_output_original': io_raw, 'input_output': io,
            'input_output_sha256': original.digest(io), 'reference_policy': original.REFERENCE_POLICY,
            'reference_index': 0, 'reference': references[0],
            'reference_sha256': original.raw_sha(references[0].encode()),
            'references_original_sha256': original.raw_sha(references_raw.encode()),
            'structurally_eligible': True, 'source_admitted': False, 'independent_group_proven': False}
