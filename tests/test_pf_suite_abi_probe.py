import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-pf-suite-abi-probe.ps1"
RESULT = ROOT / "target" / "pf-suite-abi-probe-build" / "pf-suite-abi.json"
MINIHOST = source_owners.L2_MAIN
SUITE_ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"


def test_pf_suite_abi_probe_builds_and_records_sdk_layouts():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    report = json.loads(RESULT.read_text(encoding="utf-8"))

    assert report["schema_version"] == 1
    assert report["source_kind"] == "compiled_sdk_header_observation"
    assert report["scalars"]["pointer"] == 8
    assert report["scalars"]["A_long"] == 4
    assert report["scalars"]["PF_Err"] == 4
    assert report["scalars"]["PF_Boolean"] == 1
    assert report["scalars"]["AEGP_FrameReceiptH"] == report["scalars"]["pointer"]
    assert report["scalars"]["AEGP_WorldH"] == report["scalars"]["pointer"]
    assert report["suite_versions"]["kAEGPRenderSuiteVersion4"] == 5
    assert report["suite_versions"]["kAEGPRenderAsyncManagerSuiteVersion1"] == 1

    expected = {
        "PF_WorldTransformSuite1": 7,
        "PF_PathDataSuite1": 11,
        "AEGP_RenderOptionsSuite1": 17,
        "AEGP_WorldSuite3": 13,
        "AEGP_RenderSuite4": 12,
        "AEGP_RenderAsyncManagerSuite1": 2,
    }
    for name, slots in expected.items():
        suite = report["suites"][name]
        assert suite["named_slot_count"] == slots
        assert suite["size"] == slots * report["scalars"]["pointer"]
        assert suite["alignment"] == report["scalars"]["pointer"]
        members = list(suite["members"].values())
        assert len(members) == slots
        assert [entry["offset"] for entry in members] == [
            index * report["scalars"]["pointer"] for index in range(slots)
        ]
        assert all(entry["size"] == report["scalars"]["pointer"] for entry in members)




def test_pf_suite_abi_probe_records_receipt_suite_member_names():
    report = json.loads(RESULT.read_text(encoding="utf-8"))

    assert list(report["suites"]["AEGP_RenderSuite4"]["members"]) == [
        "AEGP_RenderAndCheckoutFrame",
        "AEGP_RenderAndCheckoutLayerFrame",
        "AEGP_CheckinFrame",
        "AEGP_GetReceiptWorld",
        "AEGP_GetRenderedRegion",
        "AEGP_IsRenderedFrameSufficient",
        "AEGP_RenderNewItemSoundData",
        "AEGP_GetCurrentTimestamp",
        "AEGP_HasItemChangedSinceTimestamp",
        "AEGP_IsItemWorthwhileToRender",
        "AEGP_CheckinRenderedFrame",
        "AEGP_GetReceiptGuid",
    ]
    assert list(report["suites"]["AEGP_RenderAsyncManagerSuite1"]["members"]) == [
        "AEGP_CheckoutOrRender_ItemFrame_AsyncManager",
        "AEGP_CheckoutOrRender_LayerFrame_AsyncManager",
    ]


