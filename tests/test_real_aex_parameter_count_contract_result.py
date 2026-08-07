import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
def test_real_aex_parameter_count_contract_result():
    result = json.loads(
        (ROOT / "analysis/REAL_AEX_PARAMETER_COUNT_CONTRACT_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )
    assert result["sdk_contract"]["implicit_input_layer_included"]
    assert result["sdk_contract"]["out_data_count_must_match_add_param_calls_plus_one"]
    assert result["sdk_contract"]["mismatch_rejected_before_lifecycle_or_render"]
    for name in ("resizer_classic", "patharray_classic", "patharray_smartfx"):
        case = result[name]
        assert case["passed"]
        assert case["in_data_num_params"] == case["explicit_parameter_count"] + 1
    assert result["invariants"]["broker_matches_interactive_descriptor_count"]
    assert result["invariants"]["classic_and_smartfx_share_count_contract"]
