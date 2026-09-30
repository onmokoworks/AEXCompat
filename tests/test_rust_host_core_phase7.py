import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
STANDALONE_GATE = (
    ROOT / "tools/test-rust-host-core-scene-topology-snapshot.ps1"
)


class RustHostCorePhase7Tests(unittest.TestCase):
    @staticmethod
    def _command_failure(result: subprocess.CompletedProcess) -> str:
        stdout = (result.stdout or b"").decode("utf-8", errors="replace")
        stderr = (result.stderr or b"").decode("utf-8", errors="replace")
        return f"exit={result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"






    @unittest.skipUnless(
        os.name == "nt",
        "the topology dual-run requires the Windows MSVC/SEH boundary",
    )
    def test_native_gate_compiles_and_runs_standalone(self):
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue632-native-"
        ) as temporary:
            result = subprocess.run(
                [
                    "powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(STANDALONE_GATE),
                    "-BuildDirectory",
                    temporary,
                ],
                cwd=ROOT,
                capture_output=True,
                timeout=240,
            )
            self.assertEqual(
                result.returncode, 0, self._command_failure(result)
            )
            reports = [
                json.loads(line)
                for line in result.stdout.decode(
                    "utf-8", errors="replace"
                ).splitlines()
                if line.startswith("{")
            ]
            self.assertEqual(len(reports), 1, self._command_failure(result))
            report = reports[0]
            self.assertEqual(
                report[
                    "rust_host_core_scene_topology_snapshot_dual_run"
                ],
                "passed",
            )
            self.assertGreaterEqual(report["checks"], 35)
            self.assertIs(report["cpp_registry"], True)
            self.assertIs(report["canonical"], True)
            self.assertIs(report["overflow_fail_closed"], True)


if __name__ == "__main__":
    unittest.main()
