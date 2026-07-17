import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sdk_custom_ui_idle_runs_inside_a_complete_context_lifecycle() -> None:
    result = json.loads(
        (ROOT / "analysis" / "SDK_CUSTOM_UI_IDLE_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )

    assert result["event_sequence"] == [
        "new_context",
        "activate",
        "idle",
        "deactivate",
        "close_context",
    ]
    for observation in (result["ccu_layer_ui"], result["colorgrid_effect_ui"]):
        assert observation["lifecycle_errors"] == [0, 0, 0, 0, 0]
        assert observation["context_stable"]
        assert observation["host_state_cleared_after_close"]
        assert observation["handle_lifetimes_balanced"]
        assert observation["suite_leases_balanced"]
    assert result["colorgrid_effect_ui"]["arbitrary_values_disposed"]
