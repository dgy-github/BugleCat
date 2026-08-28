"""Owned subprocess execution for Forge.

Timeouts terminate the descendant tree, not only the immediate CLI process.
Captured output remains with the caller and is never printed here.
"""
from __future__ import annotations

import os
import signal
import subprocess
from typing import Any


def run_owned(args: list[str], *, timeout: float, input: str | None = None,
              **kwargs: Any) -> subprocess.CompletedProcess[str]:
    options = dict(kwargs)
    if options.pop("capture_output", False):
        options["stdout"] = subprocess.PIPE
        options["stderr"] = subprocess.PIPE
    if os.name == "nt":
        options["creationflags"] = options.get("creationflags", 0) | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    proc = subprocess.Popen(args, stdin=subprocess.PIPE if input is not None else None, **options)
    try:
        stdout, stderr = proc.communicate(input=input, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        terminate_tree(proc)
        stdout, stderr = proc.communicate()
        raise subprocess.TimeoutExpired(args, timeout, output=stdout, stderr=stderr) from error
    return subprocess.CompletedProcess(args, proc.returncode, stdout, stderr)


def terminate_tree(proc: subprocess.Popen[Any]) -> None:
    if proc.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/pid", str(proc.pid), "/t", "/f"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    else:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
