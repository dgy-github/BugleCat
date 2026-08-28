import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from process_control import run_owned


class OwnedProcessTests(unittest.TestCase):
    def test_captures_output(self):
        result = run_owned(
            [sys.executable, "-c", "print('ready')"],
            timeout=5,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), "ready")

    def test_timeout_kills_the_descendant_tree(self):
        marker = Path(tempfile.mkdtemp(prefix="forge-owned-test-")) / "orphan.txt"
        child = (
            "import time,pathlib;time.sleep(1);"
            f"pathlib.Path({str(marker)!r}).write_text('orphan',encoding='utf-8')"
        )
        parent = (
            "import subprocess,sys,time;"
            f"subprocess.Popen([sys.executable,'-c',{child!r}]);time.sleep(5)"
        )
        with self.assertRaises(subprocess.TimeoutExpired):
            run_owned(
                [sys.executable, "-c", parent],
                timeout=0.2,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
        time.sleep(1.2)
        self.assertFalse(marker.exists(), "timed-out Forge command left a grandchild alive")

    def test_forge_output_directories_can_be_scoped_by_the_host(self):
        root = Path(tempfile.mkdtemp(prefix="forge-output-test-"))
        env = dict(os.environ)
        env["NCX_FORGE_GENOMES_DIR"] = str(root / "genomes")
        env["NCX_FORGE_RUNS_DIR"] = str(root / "runs")
        result = run_owned(
            [
                sys.executable,
                "-c",
                "import forge;print(forge.GENOMES_DIR);print(forge.RUNS_DIR)",
            ],
            timeout=5,
            cwd=str(Path(__file__).resolve().parent),
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(
            result.stdout.strip().splitlines(),
            [str((root / "genomes").resolve()), str((root / "runs").resolve())],
        )

    def test_agent_binary_is_supplied_by_the_host(self):
        root = Path(tempfile.mkdtemp(prefix="forge-agent-test-"))
        agent = root / ("ncx.exe" if os.name == "nt" else "ncx")
        env = dict(os.environ)
        env["NCX_FORGE_NCX_BIN"] = str(agent)
        bench = Path(__file__).resolve().parent.parent / "bench"
        result = run_owned(
            [sys.executable, "-c", "import run;print(run.NCX)"],
            timeout=5,
            cwd=str(bench),
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), str(agent.resolve()))


if __name__ == "__main__":
    unittest.main()
