"""Behavioral tests for the private AE 2026 ``PF AE Private Effect Suite``.

Issue #1283: ``3D Camera Tracker.aex`` and ``Stabilizer.aex`` failed
PARAMS_SETUP with 13 because the host published no such suite.  Reverse
engineering ``VideoFilterHost.dll``'s ``RegisterPrivateEffectSuite``
(0x1800443d0) showed one ten-entry function-pointer table registered three
times, at versions 3, 5 and 6, with no data members.  At PARAMS_SETUP both
callers use exactly one slot: index 2, ``HostUTF16ToMultibyteString``, which
they hand the UTF-16 parameter name that ``dvacore::config::Localizer`` just
produced.

Issue #1295: past discovery, both reach index 3, ``HostZStringToUTF16String``,
during SMART_RENDER and returned its diagnosed stub's 4 as their own frame
error.  That slot performs AE's ZString split - ``$$$/path/key=Default text``
becomes ``Default text`` - which the host now implements too.

The published table is longer than AE's ten entries so a caller reading past
the tenth reaches a diagnosed stub instead of whatever follows it in memory;
every slot but 2 and 3 answers with the host's diagnosed refusal.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")


def test_native_pf_private_effect_suite_passes_all_three_workers() -> None:
    expected = {
        "pf_private_effect_suite": "passed",
        "versions": [3, 5, 6],
        "published_slots": 32,
        "host_table_slots": 10,
        "implemented_slots": [2, 3],
    }
    for name in WORKERS:
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-pf-private-effect-suite"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
