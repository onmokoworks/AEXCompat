import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_TIMED_LAYER_INPUT_RESULT_2026-07-17.json"


def test_timed_layer_runtime_result_proves_exact_selection_and_distinct_pixels():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    matching = result["matching_key"]
    nonmatching = result["nonmatching_key"]

    assert result["schema_version"] == 1
    assert result["status"] == "passed"
    assert matching["transport_key"] == "v1|6|1|2"
    assert matching["worker_exit_code"] == 0
    assert matching["render_error"] == 0
    assert matching["param_checkout_calls"] == matching["param_checkin_calls"] == 1
    assert matching["last_param_checkout_index"] == result["fixture"]["layer_slot"]
    assert matching["last_param_checkout_time"] == result["fixture"]["render_time"]
    assert matching["last_param_checkout_time_scale"] == result["fixture"]["render_time_scale"]
    assert nonmatching["param_checkout_calls"] == 0
    assert matching["worker_output_sha256"] != nonmatching["worker_output_sha256"]
    assert matching["output_file_sha256"] != nonmatching["output_file_sha256"]
    assert all(result["assertions"].values())
