import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_RENDER_PATH_NEGOTIATION_RESULT_2026-07-15.json"
RENDER_REPORT = ROOT / "minihost" / "src" / "worker_render_report.cpp"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"

def test_classic_only_sdk_effects_are_capability_gated_before_smart_dispatch():
    result = json.loads(RESULT.read_text(encoding="utf-8"))

    assert result["result"] == "non_advertised_smart_selectors_are_not_dispatched"
    for fixture in result["fixtures"]:
        assert fixture["smart_render_advertised"] is False
        assert fixture["classic_applicable_formats"] == ["argb8", "argb16"]
        assert fixture["classic_passed"] == 2
        assert fixture["smart_cases_not_dispatched"] == 3

    gate = result["gate"]
    assert gate["advertisement_flag"] == "PF_OutFlag2_SUPPORTS_SMART_RENDER"
    assert gate["advertisement_bit"] == 10
    assert gate["checked_after_global_setup"] is True
    assert gate["checked_before_smart_pre_render"] is True
    assert gate["classification"] == "unsupported_render_path"
    assert gate["failure_stage"] == "render_path_negotiation"
    assert gate["selector_error"] is None
    assert gate["non_applicable_not_counted_as_failure"] is True

def test_smart_positive_control_still_completes_all_depths_and_paths():
    control = json.loads(RESULT.read_text(encoding="utf-8"))["positive_control"]

    assert control["smart_render_advertised"] is True
    assert control["matrix_applicable"] == 6
    assert control["matrix_passed"] == 6
    assert control["matrix_failed"] == 0

