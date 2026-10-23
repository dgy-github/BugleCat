"""Mine SWE-bench-style replay tasks from a repo's "fixes #N" commits.

No model calls. For each candidate commit:
  1. require both source and test changes (few source files);
  2. fetch the referenced GitHub issue (skip if it is actually a PR);
  3. verify locally: with the commit's test files applied on the parent,
     some tests fail; on the commit itself they pass (fail_to_pass).
Outputs tasks.jsonl sorted by issue creation time, plus a rejects log.

Usage:
  python evals/compounding_replay/mine_tasks.py            # sqlparse defaults
  set GITHUB_TOKEN=... to raise the API rate limit (60/h unauthenticated).
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX_RE = re.compile(r"\bfix(?:es|ed)?\s+#(\d+)", re.IGNORECASE)
BODY_RE = re.compile(r"\b(?:fix(?:es|ed)?|close[sd]?|resolve[sd]?)\s*:?\s+#(\d+)", re.IGNORECASE)


def git(repo: Path, *args: str, check: bool = True) -> str:
    out = subprocess.run(["git", "-C", str(repo), *args], capture_output=True,
                         text=True, encoding="utf-8", errors="replace")
    if check and out.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


def ensure_clone(url: str, dest: Path) -> None:
    if not (dest / ".git").exists():
        dest.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "-q", url, str(dest)], check=True)


def fetch_issue(owner_repo: str, number: int, cache: Path) -> dict | None:
    cache.mkdir(parents=True, exist_ok=True)
    f = cache / f"{number}.json"
    if f.exists():
        return json.loads(f.read_text(encoding="utf-8"))
    req = urllib.request.Request(
        f"https://api.github.com/repos/{owner_repo}/issues/{number}",
        headers={"Accept": "application/vnd.github+json", "User-Agent": "ncx-replay-miner"})
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            data = json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            data = {"missing": True}
        else:
            raise
    keep = {k: data.get(k) for k in ("number", "title", "body", "created_at",
                                     "html_url", "pull_request", "missing")}
    f.write_text(json.dumps(keep, ensure_ascii=False, indent=1), encoding="utf-8")
    return keep


def run_tests(repo: Path, test_files: list[str]) -> dict[str, str] | None:
    """Return {test_id: 'passed'|'failed'} or None if the run is unusable."""
    with tempfile.TemporaryDirectory() as td:
        xml = Path(td) / "r.xml"
        env = dict(os.environ, PYTHONPATH=str(repo), PYTHONDONTWRITEBYTECODE="1")
        try:
            subprocess.run([sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider",
                            "--tb=no", f"--junitxml={xml}", *test_files],
                           cwd=repo, env=env, capture_output=True, timeout=300)
        except subprocess.TimeoutExpired:
            return None
        if not xml.exists():
            return None
        results: dict[str, str] = {}
        for case in ET.parse(xml).iter("testcase"):
            tid = f"{case.get('classname')}::{case.get('name')}"
            bad = any(c.tag in ("failure", "error") for c in case)
            skipped = any(c.tag == "skipped" for c in case)
            if not skipped:
                results[tid] = "failed" if bad else "passed"
        return results


def candidates(repo: Path, since: str, body: bool = False) -> list[tuple[str, str, int]]:
    """(sha, subject, issue) for commits referencing an issue.

    Default: non-merge commits whose subject says "fix(es|ed) #N" (sqlparse
    convention). With `body`, the full message is searched for fix/close/resolve
    keywords and merge commits are included (diffed against their first parent),
    which catches fixes landed via pull requests.
    """
    args = ["log", f"--since={since}", "--pretty=format:%H%x1f%s%x1f%B%x1e"]
    if not body:
        args.insert(1, "--no-merges")
    out, seen = [], set()
    for rec in git(repo, *args).split("\x1e"):
        parts = rec.strip("\n").split("\x1f")
        if len(parts) < 3:
            continue
        sha, subject, message = parts[0].strip(), parts[1], parts[2]
        text = message if body else subject
        rx = BODY_RE if body else FIX_RE
        for m in rx.finditer(text):
            num = int(m.group(1))
            if (sha, num) not in seen:
                seen.add((sha, num))
                out.append((sha, subject, num))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo-url", default="https://github.com/andialbrecht/sqlparse.git")
    ap.add_argument("--owner-repo", default="andialbrecht/sqlparse")
    ap.add_argument("--src-prefix", default="sqlparse/")
    ap.add_argument("--test-prefix", default="tests/")
    ap.add_argument("--since", default="2016-01-01")
    ap.add_argument("--max-src-files", type=int, default=3)
    ap.add_argument("--body", action="store_true",
                    help="search full commit messages (fix/close/resolve #N) and include merge commits")
    ap.add_argument("--out", default=str(HERE / "tasks" / "sqlparse.jsonl"))
    args = ap.parse_args()

    name = args.owner_repo.split("/")[1]
    repo = HERE / ".cache" / name
    ensure_clone(args.repo_url, repo)
    head = git(repo, "rev-parse", "HEAD").strip()

    tasks, rejects = [], []
    try:
        for sha, subject, num in candidates(repo, args.since, args.body):
            def reject(reason: str) -> None:
                rejects.append({"sha": sha[:10], "issue": num, "subject": subject, "reason": reason})
                print(f"  - #{num} {sha[:8]} rejected: {reason}")

            if any(t["issue_number"] == num for t in tasks):
                reject("issue already mined from another commit"); continue
            files = git(repo, "diff", "--name-only", f"{sha}^", sha).split()
            src = [f for f in files if f.startswith(args.src_prefix) and f.endswith(".py")]
            tests = [f for f in files if f.startswith(args.test_prefix) and f.endswith(".py")]
            if not src or not tests:
                reject("needs both source and test changes"); continue
            if len(src) > args.max_src_files:
                reject(f"too many source files ({len(src)})"); continue

            issue = fetch_issue(args.owner_repo, num, HERE / ".cache" / f"{name}-issues")
            if not issue or issue.get("missing"):
                reject("issue not found"); continue
            if issue.get("pull_request"):
                reject("referenced number is a PR, not an issue"); continue
            fixed_at = git(repo, "show", "-s", "--format=%cI", sha).strip()
            created = datetime.fromisoformat(issue["created_at"].replace("Z", "+00:00"))
            if created > datetime.fromisoformat(fixed_at):
                # e.g. tabulate's pre-GitHub (Bitbucket) "fix #N" refers to a different tracker
                reject("issue created after the fix commit (number from another tracker?)"); continue

            parent = git(repo, "rev-parse", f"{sha}^").strip()
            git(repo, "checkout", "-q", "-f", parent)
            git(repo, "clean", "-q", "-fdx")
            git(repo, "checkout", sha, "--", *tests)
            before = run_tests(repo, tests)
            git(repo, "checkout", "-q", "-f", sha)
            git(repo, "clean", "-q", "-fdx")
            after = run_tests(repo, tests)
            if after is None or not after:
                reject("tests unusable on fix commit"); continue
            before = before or {}
            f2p = sorted(t for t, r in after.items() if r == "passed" and before.get(t) != "passed")
            p2p = sorted(t for t, r in after.items() if r == "passed" and before.get(t) == "passed")
            if not f2p:
                reject("no fail_to_pass tests"); continue
            if any(r != "passed" for r in after.values()):
                reject("some tests fail even on fix commit (env drift)"); continue

            tasks.append({
                "id": f"{name}-{num}",
                "repo": args.owner_repo,
                "issue_number": num,
                "created_at": issue["created_at"],
                "issue_url": issue["html_url"],
                "problem": f"{issue['title']}\n\n{issue.get('body') or ''}".strip(),
                "base_commit": parent,
                "fix_commit": sha,
                "test_files": tests,
                "src_files_in_fix": src,
                "fail_to_pass": f2p,
                "pass_to_pass": p2p,
            })
            print(f"  + #{num} {sha[:8]} ok: {len(f2p)} f2p, {len(p2p)} p2p")
    finally:
        git(repo, "checkout", "-q", "-f", head, check=False)

    tasks.sort(key=lambda t: t["created_at"])
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(json.dumps(t, ensure_ascii=False) + "\n" for t in tasks), encoding="utf-8")
    out.with_suffix(".rejects.json").write_text(
        json.dumps(rejects, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"\n{len(tasks)} tasks -> {out}  ({len(rejects)} rejected)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
