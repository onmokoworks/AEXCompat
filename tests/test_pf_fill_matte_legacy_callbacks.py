import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def worker(name):
    configured = os.environ.get(f"AEXCOMPAT_{name.upper()}_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / f"aex_{name}_worker.exe",
        ROOT / "target" / "minihost-build-v18" / f"aex_{name}_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)

def test_legacy_fill_native_guards_errors_and_non_null_callbacks_in_both_workers():
    for name in ("render", "smart"):
        executable = worker(name)
        assert executable is not None, f"build aex_{name}_worker before running this test"
        completed = subprocess.run(
            [str(executable), "--self-test-pf-fill-matte-legacy"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == {
            "pf_fill_matte_legacy_callbacks": "passed"
        }
