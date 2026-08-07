import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_SUPERVISED_PARAMETER_RESULT_2026-07-15.json"
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
