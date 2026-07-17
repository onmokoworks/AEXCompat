import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_mouse_exited_is_dispatched_only_to_layer_or_comp_ui() -> None:
    result = json.loads(
        (ROOT / "analysis" / "SDK_CUSTOM_UI_MOUSE_EXITED_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )
    positive = result["positive_observation"]
    assert positive["target"] == "layer"
    assert positive["event_sequence"] == [
        "new_context",
        "activate",
        "mouse_exited",
        "deactivate",
        "close_context",
    ]
    assert positive["lifecycle_errors"] == [0, 0, 0, 0, 0]
    assert positive["context_stable"]
    assert positive["host_state_cleared_after_close"]
    assert positive["handle_lifetimes_balanced"]
    assert positive["suite_leases_balanced"]
    negative = result["negative_observation"]
    assert negative["registered_target"] == "effect_controls"
    assert not negative["native_event_dispatched"]
    assert negative["worker_exit_code"] == 19
