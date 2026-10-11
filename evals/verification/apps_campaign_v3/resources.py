"""Complete-packet resource admission before any new model response exists.

The reserve is the candidate's JSON-escaped UTF8 byte length, including quotes.
No original pair or candidate suffix is truncated. An oversize actual candidate
remains an Unknown resource outcome, never a manufactured negative.
"""
from ..apps_v3.oracle import raw_sha
from . import packet

CANDIDATE_JSON_BYTES = 16000
SCHEMA = "buglecat.verification.apps-complete-packet-resources.v3"
PROBE_CODE = "# source-only size probe; never execute"


def candidate_size(code):
    if type(code) is not str or not code.strip():
        raise ValueError("complete_candidate_code_required")
    return len(packet.canonical_bytes(code))


def inspect(task):
    """Source-only complete inputs, reserved candidate space, no reference read."""
    arguments = (task['question'], task['starter_code'], task['input_output']['fn_name'])
    body = packet.request(*arguments, PROBE_CODE, task['input_output'], task['source_kind'])
    fixed_bytes = len(packet.canonical_bytes(body)) - candidate_size(PROBE_CODE)
    prompt = packet.main_prompt(*arguments, task['source_kind'])
    return {'schema': SCHEMA, 'template_version': packet.TEMPLATE,
            'complete_non_candidate_packet_bytes': fixed_bytes,
            'candidate_json_byte_reserve': CANDIDATE_JSON_BYTES,
            'maximum_input_bytes': packet.MAX_INPUT_BYTES,
            'maximum_reserved_packet_bytes': fixed_bytes + CANDIDATE_JSON_BYTES,
            'resource_qualified': fixed_bytes + CANDIDATE_JSON_BYTES <= packet.MAX_INPUT_BYTES,
            'original_pair_count': len(task['input_output']['inputs']),
            'original_pairs_truncated': False, 'reference_or_candidate_executed': False,
            'main_prompt_sha256': raw_sha(prompt.encode()),
            'main_prompt_bytes': len(prompt.encode()), 'candidate_label': None,
            'source_only': True, 'collection_authorized': False, 'paid_calls': 0}


def checked_request(task, original_candidate_code):
    """Later intake must supply original exact code and preserve failed size gates."""
    resource = inspect(task)
    if not resource['resource_qualified']:
        raise ValueError("source_complete_packet_reserve_exceeded")
    if candidate_size(original_candidate_code) > CANDIDATE_JSON_BYTES:
        raise ValueError("original_candidate_json_byte_reserve_exceeded")
    body = packet.request(task['question'], task['starter_code'], task['input_output']['fn_name'],
                          original_candidate_code, task['input_output'], task['source_kind'])
    if len(packet.canonical_bytes(body)) > resource['maximum_reserved_packet_bytes']:
        raise ValueError("complete_packet_resource_contract_changed")
    return body
