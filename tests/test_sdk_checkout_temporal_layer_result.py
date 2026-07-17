import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_CHECKOUT_TEMPORAL_LAYER_RESULT_2026-07-15.json"
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def test_checkout_fixture_observes_temporal_layer_composition():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    render = result["render"]

    assert result["result"] == "render_completed"
    assert render["checkout_slot"] == 2
    assert render["checkout_time"] == (
        render["current_time"] + render["frame_offset"] * render["time_step"]
    )
    assert render["checkout_calls"] == render["checkin_calls"] == 1
    assert render["wide_time_checkout_allowed"] is True
    assert render["rejected_temporal_param_checkouts"] == 0
    assert render["param_checkouts_balanced"] is True
    assert render["top_half_is_checked_out_layer"] is True
    assert render["bottom_half_is_input_top_half"] is True
    assert render["guard_bytes_intact"] is True
    assert render["suite_leases_balanced"] is True


def test_checkout_host_surface_remains_bounded_and_observable():
    source = SOURCE.read_text(encoding="utf-8")

    for marker in (
        '"PF Effect UI Suite"',
        '"PF AE Channel Suite"',
        "param_checkouts_balanced",
        "last_param_checkout_time",
        "channel_count_queries",
        "options_button_name",
    ):
        assert marker in source
