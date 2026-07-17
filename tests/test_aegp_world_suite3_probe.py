import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-aegp-world-suite3-probe.ps1"
RESULT = ROOT / "target" / "aegp-world-suite3-probe-build" / "aegp-world-suite3-result.json"
SOURCE = ROOT / "instruments" / "aegp-world-suite3-probe" / "main.cpp"


def test_aegp_world_suite3_probe_builds_and_exercises_read_slots():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    report = json.loads(RESULT.read_text(encoding="utf-8"))
    assert report["source_kind"] == "native_sdk_abi_fixture"
    assert report["suite"] == "AEGP_WorldSuite3"
    assert report["slots_exercised"] == [2, 3, 4, 5, 6, 7, 8]
    assert report["passed"] is True
    assert [item["type"] for item in report["observations"]] == [1, 2, 3]
    assert all(item["typed_address_matches"] for item in report["observations"])
    assert all(item["wrong_typed_addresses_rejected"] for item in report["observations"])
    assert all(item["projection_matches"] for item in report["observations"])
    assert len({item["pixel_hash"] for item in report["observations"]}) == 3


def test_probe_is_sdk_typed_and_scoped_to_world_suite_read_operations():
    source = SOURCE.read_text(encoding="utf-8")
    assert '#include "AE_GeneralPlug.h"' in source
    assert "sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*)" in source
    for callback in (
        "AEGP_GetType", "AEGP_GetSize", "AEGP_GetRowBytes", "AEGP_GetBaseAddr8",
        "AEGP_GetBaseAddr16", "AEGP_GetBaseAddr32", "AEGP_FillOutPFEffectWorld",
    ):
        assert callback in source
    assert "AEGP_New(" not in source
    assert "AEGP_Dispose(" not in source
