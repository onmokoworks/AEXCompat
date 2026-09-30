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

The native self-test acquires versions 5, 8, 11, 14, and 15. It compares four slot
pairs straddling both insertions, requires every unclaimed version 5 slot to be
exactly the unsupported stub, checks the complete public Suite3 version 8 slot
map, and pins legacy GetLayerName slots to their exact stubs so the later
four-argument form cannot be copied into a three-argument slot. Version 15 is
acquired through the production path with render receipts both absent and
present; both states must expose the same complete table.
"""

from pathlib import Path

from _native_selftest import worker_self_test


ROOT = Path(__file__).resolve().parents[1]


def test_native_layer_suite1_slot_map_passes_all_three_workers() -> None:
    expected = {"aegp_layer_suite1_slots": "passed"}
    for report in worker_self_test("--self-test-aegp-layer-suite1").values():
        assert report == expected
