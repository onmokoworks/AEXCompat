import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-pf-color-settings-abi-probe.ps1"
RESULT = ROOT / "target" / "pf-color-settings-abi-probe-build" / "pf-color-settings-abi.json"
SOURCE = ROOT / "instruments" / "pf-color-settings-abi-probe" / "main.cpp"

MEMBERS = [
    "AEGP_GetBlendingTables", "AEGP_DoesViewHaveColorSpaceXform",
    "AEGP_XformWorkingToViewColorSpace", "AEGP_GetNewWorkingSpaceColorProfile",
    "AEGP_GetNewColorProfileFromICCProfile", "AEGP_GetNewICCProfileFromColorProfile",
    "AEGP_GetNewColorProfileDescription", "AEGP_DisposeColorProfile",
    "AEGP_GetColorProfileApproximateGamma", "AEGP_IsRGBColorProfile",
    "AEGP_SetWorkingColorSpace", "AEGP_IsOCIOColorManagementUsed",
    "AEGP_GetOCIOConfigurationFile", "AEGP_GetOCIOConfigurationFilePath",
    "AEGPD_GetOCIOWorkingColorSpace", "AEGPD_GetOCIODisplayColorSpace",
    "AEGPD_IsColorSpaceAwareEffectsEnabled", "AEGPD_GetLUTInterpolationMethod",
    "AEGPD_GetGraphicsWhiteLuminance", "AEGPD_GetWorkingColorSpaceId",
]


def load_report():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_color_settings_suite6_compiles_against_sdk_and_records_all_slots():
    report = load_report()
    assert report["schema_version"] == 1
    assert report["source_kind"] == "compiled_sdk_header_observation"
    assert report["architecture"] == "x86_64-windows"
    assert report["acquisition"] == {"name": "PF Color Settings Suite", "version": 7}
    suite = report["suite"]
    assert suite["name"] == "AEGP_ColorSettingsSuite6"
    assert suite["slot_count"] == 20
    assert suite["size"] == 160
    assert suite["alignment"] == 8
    assert list(suite["members"]) == MEMBERS
    assert [entry["offset"] for entry in suite["members"].values()] == list(range(0, 160, 8))
    assert all(entry["size"] == 8 for entry in suite["members"].values())
    assert all(entry["type_matches"] for entry in suite["members"].values())


def test_color_settings_probe_records_related_sdk_type_sizes():
    report = json.loads(RESULT.read_text(encoding="utf-8"))
    types = report["types"]
    assert types["pointer"] == 8
    assert types["A_Err"] == 4
    assert types["A_long"] == 4
    assert types["A_Boolean"] == 1
    assert types["A_FpShort"] == 4
    assert types["A_u_short"] == 2
    assert types["AEGP_PluginID"] == 4
    for name in (
        "PR_RenderContextH", "PF_EffectBlendingTables", "AEGP_ItemViewP",
        "AEGP_WorldH", "AEGP_CompH", "AEGP_ColorProfileP",
        "AEGP_ConstColorProfileP", "AEGP_MemHandle", "AEGP_GuidP",
    ):
        assert types[name] == types["pointer"]


