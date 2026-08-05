import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_TRANSFORMER_MULTI_INPUT_RESULT_2026-07-15.json"
def test_transformer_observes_multi_input_difference_render():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    default = result["default_render"]
    external = result["external_layer_render"]

    assert result["result"] == "classic_multi_input_render_completed"
    assert result["parameter_observation"]["layer_default"] == -1
    assert default["pixel_formats"] == ["argb8", "argb16"]
    assert default["difference_rgb_oracle_exact"] is True
    assert default["alpha_preserved"] is True
    assert default["abort_calls"] == 6
    assert default["checkout_calls"] == default["checkin_calls"] == 1
    assert default["worlds_created"] == default["worlds_disposed"] == 1
    for ownership in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "param_checkouts_balanced",
    ):
        assert default[ownership] is True
    assert external["rgba_oracle_exact"] is True
    assert external["checkout_time"] == external["current_time"]

