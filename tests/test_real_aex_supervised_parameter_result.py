import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_SUPERVISED_PARAMETER_RESULT_2026-07-15.json"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"


def test_particlelab_standard_supervised_parameter_receives_current_value():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    assert result["observed_supervised_parameters"]["standard_parameter_count"] == 7
    assert result["dispatch"]["slot"] == 8
    assert result["dispatch"]["kind"] == "integer"
    assert result["dispatch"]["requested_value"] == 2
    assert result["dispatch"]["value_echo_matched"] is True
    assert result["dispatch"]["user_changed_param_error"] == 0
    assert result["dispatch"]["update_params_ui_error"] == 0
    assert all(value is True for value in result["invariants"].values())
    assert all(value == 0 for key, value in result["lifecycle"].items() if key.endswith("_error"))


def test_supervised_transport_is_typed_isolated_and_not_button_limited():
    harness = HARNESS.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    worker = WORKER.read_text(encoding="utf-8")
    assert '"--trigger-experimental-request"' in harness
    assert "parameter.supervised" in harness
    assert "apply_dynamic_ui_report" in harness
    assert "parameter.slot == slot && parameter.supervised" in broker
    assert "encode_interactive_payload(parameters)?" in broker
    assert "g_user_changed_parameters" in worker
    assert "apply_requested_assignments(lifecycle_definitions" in worker
    assert "g_params[offset].type != 15" not in worker
