import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-aegp-world-suite3-probe.ps1"
RESULT = ROOT / "target" / "aegp-world-suite3-probe-build" / "aegp-world-suite3-result.json"


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


