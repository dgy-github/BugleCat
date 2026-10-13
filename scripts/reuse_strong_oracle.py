"""Option 1: reuse already-paid main-model candidates, re-judge with stronger tests.

Free / local only. No main-model or Jev calls.

For each binary-labeled candidate (old APPS 316 track + STDIN cal13 + STDIN dev8):
  1. Pick up to 4 APPS reference solutions that pass the task's original tests (need >= 3).
  2. Mutate original inputs; keep a new input only if all chosen references run OK and agree.
  3. Re-run the candidate on originals (harness parity guard) + accepted new inputs.
  4. originally-supported candidate failing any accepted new input -> flipped to unsupported.

Usage: python scripts/reuse_strong_oracle.py [--limit N] [--workers W]
"""
import argparse
import copy
import hashlib
import json
import os
import random
import re
import subprocess
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TRAIN = ROOT / 'reports/.nanocodex/artifact-v3-apps-census-20261011/train.jsonl'
OLD = ROOT / 'reports/.nanocodex/artifact-v3-apps-formal316-20261011/calibration-independent-dataset.json'
CAL = ROOT / 'reports/.nanocodex/artifact-v3-apps-stdin-calibration-20261012/interim-oracle-peek.json'
DEV = ROOT / 'reports/.nanocodex/artifact-v3-apps-stdin-development-20261012/independent-development-dataset.json'
DEV_CALL = ROOT / 'reports/.nanocodex/artifact-v3-apps-development-20261011/independent-development-dataset.json'
OUT = ROOT / 'reports/.nanocodex/reuse-strong-oracle-20261013'

TARGET_NEW = 12
MAX_TRIALS = 60
MIN_REFS = 3
MAX_REFS = 4
MAX_REF_SCAN = 10
MAX_ORIG_PAIRS = 10
TIMEOUT = 4.0
MAX_INPUT_BYTES = 20000

PRELUDE = (
    'import sys, math, collections, itertools, heapq, bisect, functools, re, string, random, copy\n'
    'from typing import *\nfrom collections import *\nfrom itertools import *\nfrom functools import *\n'
    'from heapq import *\nfrom bisect import *\nfrom math import *\n'
)

CALL_HARNESS = r'''
import sys, json, copy
def norm(v):
    if isinstance(v, tuple): v = list(v)
    if isinstance(v, list): return [norm(x) for x in v]
    if isinstance(v, dict): return {str(k): norm(x) for k, x in v.items()}
    if isinstance(v, (set, frozenset)): return sorted((norm(x) for x in v), key=repr)
    if isinstance(v, (int, float, str, bool)) or v is None: return v
    return repr(v)
p = json.loads(sys.stdin.read())
ns = {'__name__': '__candidate__'}
try:
    exec(compile(p['prelude'] + p['code'], 'candidate', 'exec'), ns)
    fn = p['fn_name']
    if 'Solution' in ns and hasattr(ns['Solution'], fn):
        f = getattr(ns['Solution'](), fn)
    else:
        f = ns[fn]
except Exception as e:
    print(json.dumps({'load_error': type(e).__name__})); sys.exit(0)
out = []
for args in p['inputs']:
    try:
        out.append({'ok': True, 'v': norm(f(*copy.deepcopy(args)))})
    except Exception as e:
        out.append({'ok': False, 'e': type(e).__name__})
sys.stdout.write(json.dumps(out, default=repr))
'''


# ---------------------------------------------------------------- comparison
def feq(a, b):
    if isinstance(a, bool) or isinstance(b, bool):
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return a == b or abs(a - b) <= 1e-6 * max(1.0, abs(a), abs(b))
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(feq(x, y) for x, y in zip(a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(feq(a[k], b[k]) for k in a)
    return a == b


def call_expected_match(got, expected):
    if feq(got, expected):
        return True
    return isinstance(expected, list) and len(expected) == 1 and feq(got, expected[0])


def tokens(text):
    return str(text).split()


def stdin_expected(out):
    return '\n'.join(map(str, out)) if isinstance(out, list) else str(out)


# ---------------------------------------------------------------- execution
def run_stdin(path, text):
    t0 = time.time()
    try:
        r = subprocess.run([sys.executable, '-X', 'utf8', str(path)], input=text.encode('utf-8'),
                           capture_output=True, timeout=TIMEOUT, cwd=path.parent)
    except subprocess.TimeoutExpired:
        return {'ok': False, 'e': 'timeout', 't': TIMEOUT}
    dt = time.time() - t0
    if r.returncode != 0:
        return {'ok': False, 'e': f'exit{r.returncode}', 't': dt}
    return {'ok': True, 'v': r.stdout.decode('utf-8', 'replace'), 't': dt}


def run_call(harness, code, fn_name, inputs):
    payload = json.dumps({'prelude': PRELUDE, 'code': code, 'fn_name': fn_name, 'inputs': inputs}, default=repr)
    try:
        r = subprocess.run([sys.executable, '-X', 'utf8', str(harness)], input=payload.encode('utf-8'),
                           capture_output=True, timeout=TIMEOUT + 1.0 * len(inputs), cwd=harness.parent)
    except subprocess.TimeoutExpired:
        if len(inputs) == 1:
            return [{'ok': False, 'e': 'timeout'}]
        return [run_call(harness, code, fn_name, [x])[0] for x in inputs]
    try:
        res = json.loads(r.stdout.decode('utf-8', 'replace') or 'null')
    except Exception:
        res = None
    if not isinstance(res, list):
        return [{'ok': False, 'e': 'load_or_crash'} for _ in inputs]
    return res


# ---------------------------------------------------------------- mutation
INT_RE = re.compile(r'(?<![\w.])-?\d+(?![\w.])')


def mutate_int(x, rng):
    mode = rng.random()
    if mode < 0.5:
        y = x + rng.randint(-3, 3)
    elif mode < 0.7:
        y = x * rng.randint(2, 4)
    elif mode < 0.85:
        y = x // 2
    else:
        y = rng.choice([0, 1, 2, abs(x)])
    if x >= 0:
        y = max(0, y)
    return y


def mutate_stdin(text, rng):
    lines = text.split('\n')
    p = rng.choice([0.15, 0.3, 0.6])
    skip_first = rng.random() < 0.5
    out = []
    for i, line in enumerate(lines):
        if skip_first and i == 0:
            out.append(line)
            continue

        def rep(m):
            if rng.random() < p:
                return str(mutate_int(int(m.group()), rng))
            return m.group()
        line = INT_RE.sub(rep, line)
        if rng.random() < 0.1 and re.fullmatch(r'[A-Za-z]{2,}', line.strip() or '0'):
            chars = list(line.strip())
            rng.shuffle(chars)
            line = ''.join(chars)
        out.append(line)
    if rng.random() < 0.15 and len(out) > 3:
        body = out[1:-1] if out[-1] == '' else out[1:]
        rng.shuffle(body)
        out = [out[0]] + body + ([''] if out[-1] == '' else [])
    return '\n'.join(out)


def mutate_value(v, rng, depth=0):
    if isinstance(v, bool):
        return v
    if isinstance(v, int):
        return mutate_int(v, rng) if rng.random() < 0.6 else v
    if isinstance(v, float):
        return round(v * rng.uniform(0.5, 1.5), 6) if rng.random() < 0.6 else v
    if isinstance(v, str):
        if not v or rng.random() < 0.4:
            return v
        chars = list(v)
        op = rng.random()
        if op < 0.4:
            rng.shuffle(chars)
        elif op < 0.7 and len(chars) > 1:
            del chars[rng.randrange(len(chars))]
        else:
            chars.insert(rng.randrange(len(chars) + 1), rng.choice(chars))
        return ''.join(chars)
    if isinstance(v, list):
        items = [mutate_value(x, rng, depth + 1) for x in v]
        op = rng.random()
        if op < 0.25:
            rng.shuffle(items)
        elif op < 0.4 and items:
            del items[rng.randrange(len(items))]
        elif op < 0.55 and items:
            items.insert(rng.randrange(len(items) + 1), copy.deepcopy(rng.choice(items)))
        return items
    if isinstance(v, dict):
        return {k: mutate_value(x, rng, depth + 1) for k, x in v.items()}
    return v


# ---------------------------------------------------------------- per task
def load_solutions(row):
    s = row.get('solutions')
    if isinstance(s, str):
        try:
            s = json.loads(s)
        except Exception:
            s = []
    return s if isinstance(s, list) else []


def judge_task(job):
    task, cand = job['task'], job['candidate']
    rng = random.Random(int(hashlib.sha256(f"{cand['task_id']}".encode()).hexdigest()[:8], 16))
    io = json.loads(task['input_output'])
    inputs = io.get('inputs', [])[:MAX_ORIG_PAIRS]
    outputs = io.get('outputs', [])[:MAX_ORIG_PAIRS]
    is_call = bool(io.get('fn_name'))
    fn_name = io.get('fn_name')
    result = {**{k: cand[k] for k in ('origin', 'id', 'task_id', 'orig_label')}, 'kind': 'call' if is_call else 'stdin',
              'orig_pairs': len(io.get('inputs', []))}
    with tempfile.TemporaryDirectory(prefix='strong-oracle-') as tmp:
        tmp = Path(tmp)
        harness = tmp / 'harness.py'
        harness.write_text(CALL_HARNESS, encoding='utf-8')

        def runner(code, name):
            if is_call:
                return lambda xs: run_call(harness, code, fn_name, xs)
            path = tmp / f'{name}.py'
            path.write_text(code, encoding='utf-8')
            return lambda xs: [run_stdin(path, stdin_expected(x)) for x in xs]

        def matches_original(res):
            if len(res) != len(outputs):
                return False
            for r, exp in zip(res, outputs):
                if not r.get('ok'):
                    return False
                ok = call_expected_match(r['v'], exp) if is_call else tokens(r['v']) == tokens(stdin_expected(exp))
                if not ok:
                    return False
            return True

        # 1. references that pass originals
        refs = []
        for i, sol in enumerate(load_solutions(task)[:MAX_REF_SCAN]):
            fn = runner(sol, f'ref{i}')
            if matches_original(fn(inputs)):
                refs.append(fn)
            if len(refs) >= MAX_REFS:
                break
        result['refs_used'] = len(refs)
        if len(refs) < MIN_REFS:
            result.update(new_label=cand['orig_label'], status='insufficient_references', new_tests=0)
            return result

        # 2. candidate harness-parity on originals
        cand_fn = runner(cand['code'], 'candidate')
        orig_ok = matches_original(cand_fn(inputs))
        result['candidate_passes_originals_here'] = orig_ok
        if cand['orig_label'] == 'supported' and not orig_ok:
            result.update(new_label='supported', status='harness_mismatch_excluded', new_tests=0)
            return result

        # 3. generate agreed new inputs
        seen = {json.dumps(x, sort_keys=True, default=repr) for x in io.get('inputs', [])}
        accepted = []
        for _ in range(MAX_TRIALS):
            if len(accepted) >= TARGET_NEW:
                break
            base = copy.deepcopy(rng.choice(io['inputs']))
            trial = mutate_value(base, rng) if is_call else mutate_stdin(stdin_expected(base), rng)
            key = json.dumps(trial, sort_keys=True, default=repr)
            if key in seen or len(key) > MAX_INPUT_BYTES:
                continue
            seen.add(key)
            outs = [ref([trial])[0] for ref in refs]
            if not all(o.get('ok') for o in outs):
                continue
            if is_call:
                agree = all(feq(o['v'], outs[0]['v']) for o in outs[1:])
            else:
                agree = all(tokens(o['v']) == tokens(outs[0]['v']) for o in outs[1:]) and tokens(outs[0]['v'])
            if agree:
                accepted.append((trial, outs[0]['v'], max(o.get('t', 0) for o in outs)))
        result['new_tests'] = len(accepted)

        # 4. judge candidate on accepted new inputs
        if cand['orig_label'] == 'unsupported':
            result.update(new_label='unsupported', status='kept_original_negative')
            return result
        if not accepted:
            result.update(new_label='supported', status='no_new_tests')
            return result
        got = cand_fn([a[0] for a in accepted])
        fails = []
        first_fail = None
        for idx, ((trial, exp, _), r) in enumerate(zip(accepted, got)):
            if not r.get('ok'):
                fails.append(r.get('e', 'error'))
            elif not (feq(r['v'], exp) if is_call else tokens(r['v']) == tokens(exp)):
                fails.append('wrong_answer')
            else:
                continue
            if first_fail is None:
                first_fail = idx
        result['new_failures'] = dict(Counter(fails))
        if fails:
            result.update(new_label='unsupported', status='flipped_by_new_tests')
            result['flip_only_timeouts'] = all(f == 'timeout' for f in fails)
            result['example_failing_input'] = json.dumps(accepted[first_fail][0], default=repr)[:500]
            result['example_expected'] = json.dumps(accepted[first_fail][1], default=repr)[:300]
        else:
            result.update(new_label='supported', status='still_passes')
        return result


# ---------------------------------------------------------------- main
def load_candidates():
    cands = []
    old = json.loads(OLD.read_text(encoding='utf-8'))
    for c in old['cases']:
        if c.get('oracle_file') and Path(c['oracle_file']).exists():
            ora = json.loads(Path(c['oracle_file']).read_text(encoding='utf-8'))
            code = ora.get('original_code_span', {}).get('code')
            if not code:
                continue
            if c['label'] in ('supported', 'unsupported'):
                cands.append({'origin': 'old_apps316', 'id': c['id'], 'task_id': c['task_id'], 'orig_label': c['label'], 'code': code})
            elif c['label'] == 'unknown' and c.get('main_status') == 'ok':
                cands.append({'origin': 'old_apps316_ast', 'id': c['id'], 'task_id': c['task_id'], 'orig_label': c['label'], 'code': code})
    for origin, path, split, nested in (('stdin_cal13', CAL, 'calibration', True), ('stdin_dev8', DEV, 'development', False)):
        data = json.loads(path.read_text(encoding='utf-8'))
        data = data['dataset'] if nested else data
        for c in data['cases']:
            if c['split'] == split and c['label'] in ('supported', 'unsupported') and c.get('candidate_code_span'):
                cands.append({'origin': origin, 'id': c['id'], 'task_id': c['task_id'], 'orig_label': c['label'],
                              'code': c['candidate_code_span']['code']})
    dev_call = json.loads(DEV_CALL.read_text(encoding='utf-8'))
    for c in dev_call['cases']:
        if c.get('oracle_file') and Path(c['oracle_file']).exists():
            ora = json.loads(Path(c['oracle_file']).read_text(encoding='utf-8'))
            code = ora.get('original_code_span', {}).get('code')
            if code:
                cands.append({'origin': 'apps_dev8', 'id': c['id'], 'task_id': c['task_id'], 'orig_label': c['label'], 'code': code})
    return cands


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--limit', type=int, default=None)
    ap.add_argument('--workers', type=int, default=max(2, (os.cpu_count() or 4) - 2))
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    tasks = [json.loads(line) for line in TRAIN.open(encoding='utf-8')]
    cands = load_candidates()
    if args.limit:
        cands = cands[:args.limit]
    print(f'candidates: {len(cands)}  workers: {args.workers}', flush=True)
    results, t0 = [], time.time()
    out_jsonl = OUT / ('results-limit%d.jsonl' % args.limit if args.limit else 'results.jsonl')
    with ProcessPoolExecutor(max_workers=args.workers) as pool, out_jsonl.open('w', encoding='utf-8') as fh:
        futs = {pool.submit(judge_task, {'task': tasks[c['task_id']], 'candidate': c}): c for c in cands}
        for i, fut in enumerate(as_completed(futs), 1):
            c = futs[fut]
            try:
                r = fut.result()
            except Exception as e:
                r = {**{k: c[k] for k in ('origin', 'id', 'task_id', 'orig_label')}, 'new_label': c['orig_label'],
                     'status': f'error:{type(e).__name__}:{e}'[:200]}
            results.append(r)
            fh.write(json.dumps(r, ensure_ascii=False) + '\n')
            fh.flush()
            if i % 20 == 0 or i == len(cands):
                print(f'  {i}/{len(cands)}  {time.time() - t0:.0f}s', flush=True)
    summary = {
        'candidates': len(results),
        'orig_labels': dict(Counter(r['orig_label'] for r in results)),
        'new_labels': dict(Counter(r['new_label'] for r in results)),
        'status': dict(Counter(r['status'] for r in results)),
        'flipped': sum(r['status'] == 'flipped_by_new_tests' for r in results),
        'flipped_timeout_only': sum(bool(r.get('flip_only_timeouts')) for r in results),
        'new_tests_hist': dict(Counter(min(r.get('new_tests', 0), TARGET_NEW) for r in results)),
        'by_origin_new': {o: dict(Counter(r['new_label'] for r in results if r['origin'] == o))
                          for o in sorted({r['origin'] for r in results})},
        'quota_supported_ge_150': None, 'quota_unsupported_ge_40': None,
        'elapsed_s': round(time.time() - t0, 1), 'paid_calls': 0,
    }
    summary['quota_supported_ge_150'] = summary['new_labels'].get('supported', 0) >= 150
    summary['quota_unsupported_ge_40'] = summary['new_labels'].get('unsupported', 0) >= 40
    (OUT / ('summary-limit%d.json' % args.limit if args.limit else 'summary.json')).write_text(
        json.dumps(summary, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(summary, indent=2, ensure_ascii=False))


if __name__ == '__main__':
    main()
