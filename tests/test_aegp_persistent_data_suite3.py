"""Behavioral test for the host's ``AEGP Persistent Data Suite`` version 3.

Issue #881: ``DeepGlow2.aex`` acquires the suite in SEQUENCE_SETUP and, when
the acquire fails, returns ``PF_Err_INTERNAL_STRUCT_DAMAGED`` (512) and
abandons the setup. The worker reports that as the reserved session error -47
and the broker invalidates the session, so the plug-in never renders a frame.

The native self-test drives the published table the way a plug-in does: the
blob handle and its refusal of a foreign one, the documented default
write-through on a missing key, the documented buffer-too-small answer from
``AEGP_GetString``, the "stored value is not this big, so the default is used"
answer from ``AEGP_GetData``, the caller-owned handle from
``AEGP_GetDataHandle`` and its NULL for a zero-sized value, section/key
enumeration, and deletion emptying a section out of the walk.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")


def test_native_aegp_persistent_data_suite3_passes_all_three_workers() -> None:
    expected = {"aegp_persistent_data_suite3": "passed"}
    for name in WORKERS:
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-persistent-data-suite3"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
