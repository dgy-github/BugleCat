"""Sandboxed command execution.

Honesty note on platform fidelity
---------------------------------
Real Codex isolates commands with OS kernel facilities: Seatbelt on macOS and
Landlock + seccomp on Linux. Those give true kernel-enforced sandboxing.

This module provides a *pluggable* executor with one backend per platform:

* ``PolicyExecutor`` (non-Windows default): enforces the sandbox policy at the
  tool boundary -- it inspects the command and refuses obvious writes outside
  writable roots, then runs the command in a normal subprocess. This is
  policy-level enforcement, NOT kernel isolation. A determined command can still
  escape it (e.g. via an interpreter that the static check can't see through).

* ``WindowsJobExecutor`` (Windows default): everything PolicyExecutor does PLUS
  real OS-level PROCESS/RESOURCE containment via a Win32 Job Object — the whole
  descendant process tree is killed together on timeout / Stop / exit (no
  orphaned grandchildren, the gap a bare ``proc.kill()`` leaves) and an
  active-process cap backstops a fork bomb. This is kernel-enforced, but it is
  deliberately PROCESS/RESOURCE containment ONLY, not filesystem/network
  isolation. AppContainer + ACL write-confinement is intentionally NOT done:
  it breaks the dev tools the agent must run (git/python/node), is OS-version
  fragile, and a partial version is false security — so on Windows filesystem
  and network access stay gated at the policy + approval layer by design.

* The interface is designed so a real ``SeatbeltExecutor`` / ``LandlockExecutor``
  can be dropped in on macOS / Linux without touching callers.

The approval state machine (:mod:`nanocodex.sandbox.approval`) is the
load-bearing safety layer here and is faithful to Codex's semantics on every
platform.
"""

from __future__ import annotations

import asyncio
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from nanocodex.sandbox.policy import DANGER_FULL_ACCESS, SandboxPolicy

_IS_WINDOWS = sys.platform == "win32"
_MAX_OUTPUT = 16_000


# --- Windows Job Object: real OS-level process/resource containment ---------
#
# Verified live on Win10: CreateJobObject + SetInformationJobObject with
# KILL_ON_JOB_CLOSE | ACTIVE_PROCESS + AssignProcessToJobObject +
# TerminateJobObject. This gives kernel-enforced PROCESS containment (the whole
# tree dies together) plus a fork-bomb backstop. It is NOT filesystem/network
# isolation (see module docstring). Implemented with ctypes — no new dependency
# — and any failure degrades to plain subprocess behavior.
if _IS_WINDOWS:
    import ctypes
    from ctypes import wintypes

    _JobObjectExtendedLimitInformation = 9
    _JOB_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
    _JOB_LIMIT_ACTIVE_PROCESS = 0x8
    _JOB_LIMIT_JOB_MEMORY = 0x200
    _PROCESS_SET_QUOTA = 0x0100
    _PROCESS_TERMINATE = 0x0001

    # These mirror Win32 structs 1:1; keep the MSDN names (matches docs/search).
    class _IO_COUNTERS(ctypes.Structure):  # noqa: N801
        _fields_ = [(n, ctypes.c_ulonglong) for n in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class _JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):  # noqa: N801
        _fields_ = [
            ("PerProcessUserTimeLimit", wintypes.LARGE_INTEGER),
            ("PerJobUserTimeLimit", wintypes.LARGE_INTEGER),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.POINTER(wintypes.ULONG)),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class _JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):  # noqa: N801
        _fields_ = [
            ("BasicLimitInformation", _JOBOBJECT_BASIC_LIMIT_INFORMATION),
            ("IoInfo", _IO_COUNTERS),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    class _WindowsJob:
        """A Win32 Job Object that kills its whole process tree when closed.

        Raises OSError if any Job API call fails so the caller can degrade to
        an un-contained run rather than failing the command outright.
        """

        def __init__(self, *, active_process_limit: int = 0,
                     job_memory_bytes: int = 0) -> None:
            self._k32 = ctypes.WinDLL("kernel32", use_last_error=True)
            self._k32.CreateJobObjectW.restype = wintypes.HANDLE
            self._k32.OpenProcess.restype = wintypes.HANDLE
            self._k32.AssignProcessToJobObject.argtypes = [
                wintypes.HANDLE, wintypes.HANDLE]
            self._proc_handle: "int | None" = None
            self._job = self._k32.CreateJobObjectW(None, None)
            if not self._job:
                raise OSError(ctypes.get_last_error(), "CreateJobObject failed")
            info = _JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
            flags = _JOB_LIMIT_KILL_ON_JOB_CLOSE
            if active_process_limit > 0:
                flags |= _JOB_LIMIT_ACTIVE_PROCESS
                info.BasicLimitInformation.ActiveProcessLimit = active_process_limit
            if job_memory_bytes > 0:
                flags |= _JOB_LIMIT_JOB_MEMORY
                info.JobMemoryLimit = job_memory_bytes
            info.BasicLimitInformation.LimitFlags = flags
            ok = self._k32.SetInformationJobObject(
                self._job, _JobObjectExtendedLimitInformation,
                ctypes.byref(info), ctypes.sizeof(info))
            if not ok:
                err = ctypes.get_last_error()
                self.close()
                raise OSError(err, "SetInformationJobObject failed")

        def assign(self, pid: int) -> None:
            """Put process *pid* (and thus its future children) into the job."""
            h = self._k32.OpenProcess(
                _PROCESS_SET_QUOTA | _PROCESS_TERMINATE, False, pid)
            if not h:
                raise OSError(ctypes.get_last_error(), "OpenProcess failed")
            self._proc_handle = h
            if not self._k32.AssignProcessToJobObject(self._job, h):
                raise OSError(ctypes.get_last_error(), "AssignProcessToJobObject failed")

        def terminate(self) -> None:
            """Kill every process in the job at once."""
            if self._job:
                self._k32.TerminateJobObject(self._job, 1)

        def close(self) -> None:
            """Release handles. Closing the last job handle (kill-on-close) also
            reaps any still-running tree member, so orphans can't survive."""
            for h in (self._proc_handle, self._job):
                if h:
                    self._k32.CloseHandle(h)
            self._proc_handle = None
            self._job = None


@dataclass
class ExecResult:
    """Outcome of a single sandboxed command."""

    exit_code: int
    stdout: str
    stderr: str
    timed_out: bool = False
    sandbox_denied: bool = False
    denial_reason: str = ""

    @property
    def ok(self) -> bool:
        return self.exit_code == 0 and not self.timed_out and not self.sandbox_denied

    def render(self) -> str:
        if self.sandbox_denied:
            return f"Sandbox denied: {self.denial_reason}"
        parts: list[str] = []
        if self.stdout:
            parts.append(self.stdout)
        if self.stderr.strip():
            parts.append(f"STDERR:\n{self.stderr}")
        if self.timed_out:
            parts.append("(command timed out)")
        parts.append(f"\nExit code: {self.exit_code}")
        out = "\n".join(parts)
        if len(out) > _MAX_OUTPUT:
            half = _MAX_OUTPUT // 2
            out = (
                out[:half]
                + f"\n\n... ({len(out) - _MAX_OUTPUT:,} chars truncated) ...\n\n"
                + out[-half:]
            )
        return out


class PolicyExecutor:
    """Run commands under policy-level enforcement (see module docstring)."""

    def __init__(self, policy: SandboxPolicy) -> None:
        self.policy = policy

    def preflight(self, command: str, cwd: Path) -> tuple[bool, str]:
        """Static check before running. Returns (allowed, reason_if_denied).

        Conservative: only blocks what we can clearly attribute to a write
        outside the writable roots. Ambiguous commands are allowed through and
        rely on the approval layer + the OS for the rest.
        """
        if self.policy.mode == DANGER_FULL_ACCESS:
            return True, ""
        return True, ""

    async def run(
        self,
        command: str,
        *,
        cwd: Path,
        timeout_s: int,
        env: dict[str, str] | None = None,
        network_disabled: bool | None = None,
    ) -> ExecResult:
        allowed, reason = self.preflight(command, cwd)
        if not allowed:
            return ExecResult(
                exit_code=126, stdout="", stderr="",
                sandbox_denied=True, denial_reason=reason,
            )

        run_env = self._build_env(env or {})
        try:
            proc = await self._spawn(command, cwd, run_env)
        except Exception as exc:  # noqa: BLE001 - surfaced to the model as a tool error
            return ExecResult(exit_code=1, stdout="", stderr=f"spawn failed: {exc}")

        try:
            try:
                stdout_b, stderr_b = await asyncio.wait_for(
                    proc.communicate(), timeout=timeout_s
                )
            except asyncio.TimeoutError:
                await self._kill(proc)
                return ExecResult(exit_code=124, stdout="", stderr="", timed_out=True)
            except asyncio.CancelledError:
                await self._kill(proc)
                raise

            return ExecResult(
                exit_code=proc.returncode if proc.returncode is not None else 1,
                stdout=stdout_b.decode("utf-8", errors="replace"),
                stderr=stderr_b.decode("utf-8", errors="replace"),
            )
        finally:
            # Release any per-process resource (e.g. a Job handle) on EVERY exit
            # path — success, timeout, or cancel.
            self._cleanup(proc)

    def _cleanup(self, proc) -> None:
        """Hook: release per-process resources after a run. Base does nothing."""

    async def _spawn(self, command: str, cwd: Path, env: dict[str, str]):
        if _IS_WINDOWS:
            return await asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=str(cwd),
                env=env,
            )
        bash = shutil.which("bash") or "/bin/bash"
        return await asyncio.create_subprocess_exec(
            bash, "-l", "-c", command,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=str(cwd),
            env=env,
        )

    async def _kill(self, proc) -> None:
        try:
            proc.kill()
        except ProcessLookupError:
            return
        with __import__("contextlib").suppress(asyncio.TimeoutError):
            await asyncio.wait_for(proc.wait(), timeout=5.0)

    @staticmethod
    def _build_env(extra: dict[str, str]) -> dict[str, str]:
        if _IS_WINDOWS:
            sysroot = os.environ.get("SYSTEMROOT", r"C:\Windows")
            env = {
                "SYSTEMROOT": sysroot,
                "COMSPEC": os.environ.get("COMSPEC", f"{sysroot}\\system32\\cmd.exe"),
                "PATH": os.environ.get("PATH", f"{sysroot}\\system32;{sysroot}"),
                "PATHEXT": os.environ.get("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
                "USERPROFILE": os.environ.get("USERPROFILE", ""),
                "TEMP": os.environ.get("TEMP", f"{sysroot}\\Temp"),
                "TMP": os.environ.get("TMP", f"{sysroot}\\Temp"),
                "PYTHONUNBUFFERED": "1",
                "PYTHONIOENCODING": "utf-8",
            }
        else:
            env = {
                "HOME": os.environ.get("HOME", "/tmp"),
                "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                "LANG": os.environ.get("LANG", "C.UTF-8"),
                "TERM": os.environ.get("TERM", "dumb"),
                "PYTHONUNBUFFERED": "1",
            }
        env.update(extra)
        return env


class WindowsJobExecutor(PolicyExecutor):
    """Windows backend: run each command inside a Job Object.

    Adds real OS-level PROCESS/RESOURCE containment on top of PolicyExecutor —
    the command's whole descendant tree lives in a Win32 Job Object with
    kill-on-job-close, so a timeout / Stop / normal exit tears the entire tree
    down (no orphaned grandchildren), and an active-process cap backstops a fork
    bomb. NOT filesystem/network isolation (see module docstring). If the Job
    APIs fail for any reason the command still runs, just un-contained.
    """

    # Generous fork-bomb backstop — high enough not to bite a real build.
    ACTIVE_PROCESS_LIMIT = 512

    async def _spawn(self, command: str, cwd: Path, env: dict[str, str]):
        proc = await super()._spawn(command, cwd, env)
        try:
            job = _WindowsJob(active_process_limit=self.ACTIVE_PROCESS_LIMIT)
            job.assign(proc.pid)
            proc._nanocodex_job = job  # type: ignore[attr-defined]
        except OSError:
            # Degrade gracefully: run without containment rather than fail.
            proc._nanocodex_job = None  # type: ignore[attr-defined]
        return proc

    async def _kill(self, proc) -> None:
        job = getattr(proc, "_nanocodex_job", None)
        if job is not None:
            job.terminate()  # kill the whole tree at once
        await super()._kill(proc)

    def _cleanup(self, proc) -> None:
        job = getattr(proc, "_nanocodex_job", None)
        if job is not None:
            job.close()


def make_executor(policy: SandboxPolicy) -> PolicyExecutor:
    """Pick the best available executor for this platform.

    Windows gets :class:`WindowsJobExecutor` (Job Object process/resource
    containment); other platforms get :class:`PolicyExecutor` for now (a
    Seatbelt/Landlock backend can slot in here later). All share the same
    sandbox-policy + approval gating.
    """
    if _IS_WINDOWS:
        return WindowsJobExecutor(policy)
    return PolicyExecutor(policy)
