"""`AEGP Layer Suite` version 5 is the AE 5.0 shape, not a shifted later one.

A plug-in that acquires version 5 gets `AEGP_LayerSuite1` (frozen in AE 5.0).
Version 11 inserted `AEGP_GetLayerSourceItemID` at slot 5 and
`AEGP_ConvertLayerToCompTime` at slot 35, so a version 1 slot moves by +1 from 5
through 33 and by +2 from 34 on. Wiring the later table's indices into this one
would hand the plug-in a different function at every one of them.

The suite struct number and the version it is acquired with do not line up:
`AEGP_LayerSuite5` is acquired as 11 and `Suite8` as 14. Everything here is
named after the struct so the two do not read as the same thing.

`Unmult.aex` asks for version 5 and reported `PF_Err_INTERNAL_STRUCT_DAMAGED`
while the host refused the acquire (issue #712).

The native self-test acquires versions 5, 8, 11, and 14. It compares four slot
pairs straddling both insertions, requires every unclaimed version 5 slot to be
exactly the unsupported stub, checks the complete public Suite3 version 8 slot
map, and pins legacy GetLayerName slots to their exact stubs so the later
four-argument form cannot be copied into a three-argument slot.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"


def test_native_layer_suite1_slot_map_passes_all_three_workers() -> None:
    expected = {"aegp_layer_suite1_slots": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-layer-suite1"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
