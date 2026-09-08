"""Run the authorized challenge in an existing isolated clone, keeping safe evidence."""
import argparse
import hashlib
import json
import os
import subprocess
import time
from datetime import datetime
from pathlib import Path

PROMPT = '''Self-repair challenge: fix your own bench/run.py. Failed empty responses
(empty response or 空内容) with missing expected files must trigger bounded retries.
compile_failed takes priority over missing/empty; successful output mentioning empty
response must never retry. Preserve every physical invocation in attempts with logical
attempt number and retry index; runs/passes remain logical, mean_s sums all invocation
durations per logical run, timeouts counts physical timeouts. Limit retries to
empty_response_retries (0 disables); preserve run_once tuple API. Add deterministic
offline pytest coverage for recovery/exhaustion/classification/accounting. Only edit
bench/run.py and add tests; do not edit existing tests or benchmark tasks/checkers.
Do not call live models or benchmarks from tests. Do not commit or push. Run tests
and lint then finish immediately with results. Focus on these files; avoid broad
repository exploration. You have a 600 second total turn budget.'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('workspace', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    ws = args.workspace.resolve()
    if ws == root or not (ws / '.git').exists():
        raise SystemExit('An isolated git clone is required')
    binary = root / 'rust/target/release/ncx.exe'
    routes = json.loads((Path.home() / '.nanocodex/providers.json').read_text(encoding='utf-8-sig'))
    route = next(r for r in routes if r['id'] == 'preset:deepseek')
    model = 'deepseek-v4-pro'
    if model not in route['models']:
        raise SystemExit('Requested model not in configured route')
    env = dict(os.environ)
    env.update(NANOCODEX_API_KEY=route['api_key'], DEEPSEEK_API_KEY=route['api_key'],
               NANOCODEX_BASE_URL=route['base_url'], DEEPSEEK_BASE_URL=route['base_url'],
               NANOCODEX_PROVIDER_PROTOCOL=route['protocol'], NANOCODEX_MODEL=model,
               NANOCODEX_TURN_TIMEOUT_S='600')
    baseline = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ws, text=True).strip()
    started = time.monotonic()
    print(f'Starting {model} in {ws}; turn budget 600 seconds', flush=True)
    result = subprocess.run([str(binary), '-w', str(ws), '-s', 'workspace-write',
                             '--max-iterations', '30', '--max-tool-calls', '80', PROMPT],
                            cwd=ws, env=env, capture_output=True, text=True,
                            encoding='utf-8', errors='replace')
    # Retain only the allowlisted provider metadata, not arbitrary tool/model output.
    diagnostics = []
    for line in result.stderr.splitlines():
        if line.startswith('[ncx-provider] '):
            diagnostics.append(json.loads(line.removeprefix('[ncx-provider] ')))
    record = dict(model=model, baseline=baseline, workspace=str(ws),
                  binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  elapsed_s=round(time.monotonic() - started, 2), returncode=result.returncode,
                  provider_diagnostics=diagnostics)
    path = Path(__file__).with_name('challenge_' + datetime.now().strftime('%Y%m%d_%H%M%S') + '.json')
    path.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(record, ensure_ascii=False, indent=2), flush=True)
    print(f'Evidence: {path}', flush=True)


if __name__ == '__main__':
    main()
