import importlib.util
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "diff_aex_dossiers", ROOT / "tools/diff_aex_dossiers.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def test_dossier_diff_reports_parameter_and_observation_changes():
    trace = {
        "selector": "SMART_RENDER",
        "image_sha256": "a" * 64,
        "return_value": 0,
        "events": [
            {
                "kind": "guest_call",
                "pc_rva": 10,
                "target_rva": 20,
                "observed_count": 2,
            }
        ],
    }
    before = {
        "input_png_sha256": "1" * 64,
        "parameter_values": [{"name": "Amount", "value": 1.0}],
        "render_error": 0,
        "execution_traces": [trace],
    }
    after_trace = {**trace, "events": [{**trace["events"][0], "observed_count": 5}]}
    after = {
        "input_png_sha256": "2" * 64,
        "parameter_values": [{"name": "Amount", "value": 2.0}],
        "render_error": 0,
        "execution_traces": [after_trace],
    }

    result = MODULE.build_diff(before, after)

    assert result["same_plugin_image"] is True
    assert result["parameter_values"]["before"] != result["parameter_values"]["after"]
    delta = result["trace_diffs"][0]["event_observation_deltas"][0]
    assert delta["before"] == 2
    assert delta["after"] == 5
    assert delta["delta"] == 3
