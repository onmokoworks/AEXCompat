import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_SMARTYPANTS_DYNAMIC_SMART_RESULT_2026-07-15.json"
def test_smartypants_dynamic_flags_have_a_valid_time_and_checkout_context():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    lifecycle = result["lifecycle"]

    assert result["result"] == "dynamic_flags_and_smart_image_io_completed"
    assert lifecycle["query_dynamic_flags_dispatched"] is True
    assert lifecycle["query_dynamic_flags_error"] == 0
    assert lifecycle["query_checkout_time_step"] > 0
    assert lifecycle["query_checkout_time_scale"] > 0
    assert lifecycle["sequence_setup_error"] == 0
    assert lifecycle["sequence_resetup_error"] == 0
    assert lifecycle["sequence_setdown_error"] == 0
    assert lifecycle["lifecycle_data_null"] is True

def test_smartypants_rgb_invert_is_exact_at_all_supported_cpu_depths():
    render = json.loads(RESULT.read_text(encoding="utf-8"))["render"]

    assert render["argb8_rgb_invert_oracle_exact"] is True
    assert render["argb16_rgb_invert_oracle_exact"] is True
    assert render["argb32f_rgb_invert_oracle_exact"] is True
    assert render["parameter_checkout_count"] == render["parameter_checkin_count"]
    assert render["suite_acquire_count"] == render["suite_release_count"]
    for invariant in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
        "guard_bytes_intact",
    ):
        assert render[invariant] is True

