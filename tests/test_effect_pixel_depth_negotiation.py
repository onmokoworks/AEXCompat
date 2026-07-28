import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_EFFECT_DEBUG_MATRIX_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
REPORT = ROOT / "minihost" / "src" / "worker_render_report.cpp"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


def test_real_aex_matrix_distinguishes_supported_renders_from_depth_negotiation():
    result = json.loads(RESULT.read_text(encoding="utf-8"))

    assert result["case_count"] == 72
    assert result["applicable_count"] == 52
    assert result["passed_count"] == 52
    assert result["failed_count"] == 0
    assert result["unsupported_count"] == result["unsupported_pixel_depth_count"] == 20
    assert result["selector_failure_count"] == 0
    assert result["case_count"] == result["applicable_count"] + result["unsupported_count"]
    assert result["applicable_count"] == result["passed_count"] + result["failed_count"]
    assert result["depth_negotiation"] == {
        "argb8_requires_capability_flag": False,
        "argb16_requires_out_flag_deep_color_aware": True,
        "argb32f_requires_out_flag2_float_color_aware": True,
        "unsupported_selectors_dispatched": False,
        "unsupported_cases_are_failures": False,
        "fully_depth_capable_plugins": 7,
        "eight_bpc_only_plugins": 5,
    }
    assert sorted(result["plugins"].values()).count(2) == 5
    assert sorted(result["plugins"].values()).count(6) == 7


def test_worker_gates_deep_worlds_on_the_observed_ae_capability_bits():
    source = source_owners.worker_text() + REPORT.read_text(encoding="utf-8")

    assert "kOutFlagDeepColorAware = 1u << 25" in source
    assert "kOutFlag2FloatColorAware = 1u << 12" in source
    assert "external_pixel_bytes == 8 &&" in source
    assert "external_pixel_bytes == 16 &&" in source
    assert "params_error == 0 && image_render_supported && depth_supported" in source
    assert r'\"depth_supported\"' in source


def test_harness_reports_negotiation_without_fabricating_a_selector_error():
    source = source_owners.harness_windows_text()

    assert '"unsupported_pixel_depth".to_owned()' in source
    assert 'Some("pixel_depth_negotiation".to_owned())' in source
    assert "selector_error: if unsupported_render_path || unsupported_depth" in source
    assert "AEX did not advertise support for the requested pixel depth" in source
