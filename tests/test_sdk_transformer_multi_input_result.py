import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_TRANSFORMER_MULTI_INPUT_RESULT_2026-07-15.json"
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


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


def test_transformer_callbacks_match_the_frozen_effect_abi():
    source = SOURCE.read_text(encoding="utf-8")

    assert "write(input, 24, &abort_render);" in source
    assert "write(input, 32, &report_progress);" in source
    assert 'std::strcmp(name, "PF World Suite") == 0 && version == 1' in source
    assert "g_params[slot - 1].layer_default == -1" in source
    assert "transfer_mode < 0 || transfer_mode > 38" in source
    assert "rgb_only > 1" in source
    assert "mask_world" in source
    assert "ansi_fabs" in source
