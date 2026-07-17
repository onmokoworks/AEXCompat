import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target/minihost-build-adv-time-v1"
WORKER = BUILD / "Release/aex_render_worker.exe"


def test_suite_entry_guards_and_utility13_native_contract():
    assert WORKER.exists(), "focused Adv Time test must build the release worker first"
    result = subprocess.run(
        [str(WORKER), "--self-test-suite-entry-utility13"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert json.loads(result.stdout) == {
        "suite_entry_utility13": "passed",
        "null_fail_closed": True,
        "normal_effect_available": True,
        "versions_12_14_rejected": True,
        "mask_callbacks_exposed": False,
        "suite_leases_balanced": True,
    }
