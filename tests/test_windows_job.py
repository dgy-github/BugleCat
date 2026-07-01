"""Windows Job Object executor: real OS-level process/resource containment.

The OS-specific tests are skipped off Windows; the executor-selection test runs
everywhere. These exercise the genuine Win32 path (verified live), not a mock.
"""

from __future__ import annotations

import sys

import pytest

from nanocodex.sandbox.executor import (
    PolicyExecutor,
    WindowsJobExecutor,
    make_executor,
)
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy

_WIN = sys.platform == "win32"
skip_non_win = pytest.mark.skipif(not _WIN, reason="Windows Job Object only")


def _policy(tmp_path):
    return SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)


def test_make_executor_picks_backend_per_platform(tmp_path):
    ex = make_executor(_policy(tmp_path))
    assert isinstance(ex, PolicyExecutor)  # always (the shared base)
    if _WIN:
        assert isinstance(ex, WindowsJobExecutor)
    else:
        assert type(ex) is PolicyExecutor


@skip_non_win
async def test_normal_command_runs_under_job(tmp_path):
    # A Job Object must be transparent to a normal command's output/exit code.
    ex = WindowsJobExecutor(_policy(tmp_path))
    res = await ex.run("echo hello", cwd=tmp_path, timeout_s=15)
    assert res.exit_code == 0
    assert "hello" in res.stdout
    assert res.ok


@skip_non_win
async def test_timeout_kills_and_reports(tmp_path):
    # A long command past the timeout is killed (its tree torn down) and the
    # result is flagged timed_out.
    ex = WindowsJobExecutor(_policy(tmp_path))
    res = await ex.run("ping -n 20 127.0.0.1", cwd=tmp_path, timeout_s=1)
    assert res.timed_out
    assert res.exit_code == 124


@skip_non_win
def test_job_lifecycle_create_assign_terminate(tmp_path):
    # Direct lifecycle on a real child process: create -> assign -> terminate.
    import subprocess

    from nanocodex.sandbox.executor import _WindowsJob

    job = _WindowsJob(active_process_limit=16)
    p = subprocess.Popen("ping -n 30 127.0.0.1", shell=True,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        job.assign(p.pid)
        job.terminate()
        # The terminated process exits with a non-None return code promptly.
        assert p.wait(timeout=5) is not None
    finally:
        job.close()
        if p.poll() is None:
            p.kill()
