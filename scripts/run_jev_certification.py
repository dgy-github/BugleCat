"""Run Jev verifier model on the 318 pure APPS candidates (278 supported / 40 unsupported).

Quota: >=150 supported (actual: 278), >=40 unsupported (actual: 40).
Certification bounds:
  - False block rate <= 2% (one-sided 95% upper bound)
  - Error detection rate >= 50% (one-sided 95% lower bound)

Usage: python scripts/run_jev_certification.py [--workers 8] [--dry-run]
"""
import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.error
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from scipy.stats import beta

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'scripts'))

from evals.verification.apps_stdin_v1 import packet as stdin_packet
from evals.verification.apps_campaign_v3 import packet as call_packet
import reuse_strong_oracle as rso

OUT_DIR = ROOT / 'reports/.nanocodex/reuse-strong-oracle-20261013'
JEV_CALLS_DIR = OUT_DIR / 'jev_calls'
ENDPOINT = "https://api.typesafe.ai/v1/systemone"

GRID = [round(0.10 + i * 0.05, 2) for i in range(17)]  # 0.10, 0.15, ..., 0.90


def clopper_pearson_upper(k, n, alpha=0.05):
    if k == n:
        return 1.0
    return float(beta.ppf(1 - alpha, k + 1, n - k))


def clopper_pearson_lower(k, n, alpha=0.05):
    if k == 0:
        return 0.0
    return float(beta.ppf(alpha, k, n - k + 1))


def get_cached_attempts():
    cached = {}
    # Check apps-v3-dev in evals/verification/reports/benchmark_400_budget_20261010/jev/attempts/
    for p in ROOT.glob("evals/verification/reports/benchmark_400_budget_20261010/jev/attempts/apps-v3-dev-*/raw-response.json"):
        cid = p.parent.name.replace("apps-v3-dev-", "")
        raw = json.load(p.open(encoding='utf-8'))
        noul = raw.get('answers', {}).get('acceptance_violation', {}).get('noul')
        if noul is not None:
            cached[cid] = {'noul': noul, 'raw': raw, 'source': 'apps_dev_cached'}

    # Check stdin-dev
    for p in ROOT.glob("evals/verification/reports/benchmark_400_budget_20261010/jev/stdin-development-attempts/**/raw-response.json"):
        cid = p.parent.name
        raw = json.load(p.open(encoding='utf-8'))
        noul = raw.get('answers', {}).get('acceptance_violation', {}).get('noul')
        if noul is not None:
            cached[cid] = {'noul': noul, 'raw': raw, 'source': 'stdin_dev_cached'}
    return cached


def call_jev_single(key, req_body, max_retries=3):
    headers = {
        "Authorization": f"Bearer {key}",
        "Content-Type": "application/json"
    }
    data = json.dumps(req_body, ensure_ascii=False).encode('utf-8')
    req = urllib.request.Request(ENDPOINT, data=data, headers=headers, method="POST")

    for attempt in range(max_retries):
        try:
            t0 = time.time()
            with urllib.request.urlopen(req, timeout=20) as resp:
                status = resp.status
                body = resp.read().decode('utf-8')
                elapsed = time.time() - t0
                parsed = json.loads(body)
                noul = parsed.get('answers', {}).get('acceptance_violation', {}).get('noul')
                return {'ok': True, 'noul': noul, 'raw': parsed, 'elapsed': elapsed}
        except urllib.error.HTTPError as e:
            code = e.code
            err_body = e.read().decode('utf-8', errors='ignore')
            if code == 429:
                time.sleep(2.0 * (attempt + 1))
                continue
            return {'ok': False, 'error': f"HTTP_{code}", 'detail': err_body[:200]}
        except Exception as e:
            if attempt < max_retries - 1:
                time.sleep(1.0)
                continue
            return {'ok': False, 'error': type(e).__name__, 'detail': str(e)[:200]}

    return {'ok': False, 'error': 'max_retries_exceeded'}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--workers', type=int, default=8)
    ap.add_argument('--dry-run', action='store_true')
    args = ap.parse_args()

    key = os.environ.get("NCX_JEV_API_KEY")
    if not key and not args.dry_run:
        print("ERROR: NCX_JEV_API_KEY environment variable required", file=sys.stderr)
        sys.exit(1)

    JEV_CALLS_DIR.mkdir(parents=True, exist_ok=True)

    # 1. Load train.jsonl
    print("Loading train.jsonl...", flush=True)
    train_tasks = {}
    with open(ROOT / 'reports/.nanocodex/artifact-v3-apps-census-20261011/train.jsonl', 'r', encoding='utf-8') as f:
        for line in f:
            r = json.loads(line)
            train_tasks[r['id']] = r
    print(f"Loaded {len(train_tasks)} tasks.", flush=True)

    # 2. Load candidates with code and oracle labels
    raw_cands = {c['id']: c for c in rso.load_candidates()}
    results_p = OUT_DIR / 'results.jsonl'
    candidates = []
    with open(results_p, 'r', encoding='utf-8') as f:
        for line in f:
            r = json.loads(line)
            if r['new_label'] in ('supported', 'unsupported'):
                cid = r['id']
                if cid in raw_cands:
                    cand = dict(raw_cands[cid])
                    cand['new_label'] = r['new_label']
                    candidates.append(cand)

    n_pos = sum(c['new_label'] == 'supported' for c in candidates)
    n_neg = sum(c['new_label'] == 'unsupported' for c in candidates)
    print(f"Total binary candidates: {len(candidates)} (Supported: {n_pos}, Unsupported: {n_neg})", flush=True)
    assert n_pos >= 150, f"Supported quota not met: {n_pos} < 150"
    assert n_neg >= 40, f"Unsupported quota not met: {n_neg} < 40"

    # 3. Discover cached responses
    cached = get_cached_attempts()
    print(f"Found {len(cached)} cached Jev responses.", flush=True)

    # 4. Prepare requests
    tasks_to_call = []
    ready_results = {}

    for c in candidates:
        cid = c['id']
        call_file = JEV_CALLS_DIR / f"{cid}.json"

        # If already saved in JEV_CALLS_DIR
        if call_file.exists():
            saved = json.loads(call_file.read_text(encoding='utf-8'))
            if saved.get('ok') and saved.get('noul') is not None:
                ready_results[cid] = saved
                continue

        # If available in previous cached attempts
        if cid in cached:
            item = {'ok': True, 'noul': cached[cid]['noul'], 'raw': cached[cid]['raw'], 'source': cached[cid]['source']}
            call_file.write_text(json.dumps(item, indent=2), encoding='utf-8')
            ready_results[cid] = item
            continue

        # Needs to be called
        tid = c['task_id']
        task = train_tasks[tid]
        io = json.loads(task['input_output'])
        is_call = bool(io.get('fn_name'))
        code = c['code']

        if is_call:
            req = call_packet.request(
                task['question'],
                task.get('starter_code', '') or '',
                io['fn_name'],
                code,
                io,
                'apps_call_based_solution_method'
            )
        else:
            req = stdin_packet.request(
                task['question'],
                task.get('starter_code', '') or '',
                code,
                io
            )
        tasks_to_call.append((c, req, call_file))

    print(f"Ready results: {len(ready_results)}, Jev calls to execute: {len(tasks_to_call)}", flush=True)

    if args.dry_run:
        print("Dry run complete. Exiting.")
        return

    # 5. Execute Jev calls with ThreadPoolExecutor
    t0 = time.time()
    errors = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {}
        for c, req, call_file in tasks_to_call:
            fut = pool.submit(call_jev_single, key, req)
            futures[fut] = (c, call_file)

        for i, fut in enumerate(as_completed(futures), 1):
            c, call_file = futures[fut]
            cid = c['id']
            res = fut.result()
            if res.get('ok'):
                call_file.write_text(json.dumps(res, indent=2), encoding='utf-8')
                ready_results[cid] = res
            else:
                print(f"ERROR on {cid}: {res}", flush=True)
                errors.append((cid, res))
            if i % 25 == 0 or i == len(tasks_to_call):
                print(f"  Progress: {i}/{len(tasks_to_call)} calls completed ({time.time() - t0:.1f}s)", flush=True)

    if errors:
        print(f"WARNING: {len(errors)} calls failed!", flush=True)

    print(f"\nAll Jev calls completed in {time.time() - t0:.1f}s! Total ready results: {len(ready_results)}/{len(candidates)}", flush=True)

    # 6. Statistical Certification
    pos_probs = []
    neg_probs = []
    for c in candidates:
        cid = c['id']
        r = ready_results.get(cid)
        if not r or r.get('noul') is None:
            continue
        p = r['noul']
        if c['new_label'] == 'supported':
            pos_probs.append(p)
        elif c['new_label'] == 'unsupported':
            neg_probs.append(p)

    print(f"\nScored supported: {len(pos_probs)}/{n_pos}, Scored unsupported: {len(neg_probs)}/{n_neg}", flush=True)

    cert_rows = []
    qualified_thresholds = []

    for t in GRID:
        # Decision rule: reject if p >= t
        k_pos = sum(p >= t for p in pos_probs)
        k_neg = sum(p >= t for p in neg_probs)

        # Upper bound on false block rate (want <= 0.02)
        fb_emp = k_pos / len(pos_probs) if pos_probs else 0.0
        fb_ub = clopper_pearson_upper(k_pos, len(pos_probs))

        # Lower bound on error detection rate (want >= 0.50)
        det_emp = k_neg / len(neg_probs) if neg_probs else 0.0
        det_lb = clopper_pearson_lower(k_neg, len(neg_probs))

        fb_pass = fb_ub <= 0.02
        det_pass = det_lb >= 0.50
        qualified = fb_pass and det_pass

        if qualified:
            qualified_thresholds.append(t)

        cert_rows.append({
            'threshold': t,
            'k_false_blocks': k_pos,
            'n_supported': len(pos_probs),
            'false_block_empirical': round(fb_emp, 4),
            'false_block_95_upper_bound': round(fb_ub, 4),
            'false_block_pass': fb_pass,
            'k_detections': k_neg,
            'n_unsupported': len(neg_probs),
            'detection_empirical': round(det_emp, 4),
            'detection_95_lower_bound': round(det_lb, 4),
            'detection_pass': det_pass,
            'qualified': qualified
        })

    print("\n=== THRESHOLD EVALUATION TABLE ===", flush=True)
    header = f"{'Threshold':>9} | {'FalseBlock':>10} | {'FB_UB(<=2%)':>12} | {'Pass':>5} | {'Detections':>10} | {'Det_LB(>=50%)':>13} | {'Pass':>5} | {'Qualified':>9}"
    print(header, flush=True)
    print("-" * len(header), flush=True)
    for r in cert_rows:
        print(f"{r['threshold']:>9.2f} | {r['k_false_blocks']:>3}/{r['n_supported']} ({r['false_block_empirical']*100:>4.1f}%) | {r['false_block_95_upper_bound']*100:>10.2f}% | {str(r['false_block_pass']):>5} | {r['k_detections']:>3}/{r['n_unsupported']} ({r['detection_empirical']*100:>4.1f}%) | {r['detection_95_lower_bound']*100:>11.2f}% | {str(r['detection_pass']):>5} | {str(r['qualified']):>9}", flush=True)

    report = {
        'schema': 'buglecat.verification.pure-apps-jev-certification.v1',
        'track': 'pure_apps_track',
        'policy_targets': {
            'max_false_block_upper_bound': 0.02,
            'min_error_detection_lower_bound': 0.50,
            'confidence': 0.95,
            'bound_type': 'one_sided_clopper_pearson'
        },
        'quotas': {
            'min_supported': 150,
            'actual_supported': len(pos_probs),
            'min_unsupported': 40,
            'actual_unsupported': len(neg_probs)
        },
        'calls_made': len(tasks_to_call),
        'calls_cached': len(cached),
        'total_evaluated': len(ready_results),
        'certified_qualified_thresholds': qualified_thresholds,
        'recommended_threshold': qualified_thresholds[0] if qualified_thresholds else None,
        'grid_evaluation': cert_rows
    }

    report_p = OUT_DIR / 'certification_report.json'
    report_p.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(f"\nCertification report written to: {report_p}", flush=True)
    if qualified_thresholds:
        print(f"*** CERTIFICATION SUCCESS! Qualified thresholds: {qualified_thresholds} ***", flush=True)
    else:
        print("*** CERTIFICATION FAILED: No threshold met both bounds simultaneously ***", flush=True)


if __name__ == '__main__':
    main()
