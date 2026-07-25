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


def test_trace_identity_uses_occurrence_within_each_selector():
    frame = {"selector": "FRAME_SETDOWN", "events": []}
    before = {"execution_traces": [{"selector": "SEQUENCE_SETUP"}, frame]}
    after = {
        "execution_traces": [
            {"selector": "SEQUENCE_SETUP"},
            {"selector": "SMART_RENDER"},
            frame,
        ]
    }

    result = MODULE.build_diff(before, after)
    frame_diff = next(
        item for item in result["trace_diffs"] if item["selector"] == "FRAME_SETDOWN#0"
    )

    assert frame_diff["present_before"] is True
    assert frame_diff["present_after"] is True


def test_event_counts_accumulate_duplicate_semantic_keys():
    event = {
        "kind": "guest_call",
        "depth": 2,
        "pc_rva": 10,
        "target_rva": 20,
    }
    trace = {
        "events": [
            {**event, "observed_count": 2},
            {**event, "observed_count": 3},
        ]
    }

    counts = MODULE.count_events(trace)

    assert list(counts.values()) == [5]


def test_event_key_distinguishes_call_kind():
    direct = {
        "kind": "guest_call",
        "depth": 1,
        "pc_rva": 10,
        "target_rva": 20,
        "call_kind": "direct",
    }
    indirect = {**direct, "call_kind": "indirect_register"}

    assert MODULE.event_key(direct) != MODULE.event_key(indirect)


def test_event_delta_reports_bounded_truncation():
    before = MODULE.Counter({f"before-{index}": 1 for index in range(3)})
    after = MODULE.Counter({f"after-{index}": 1 for index in range(3)})

    result = MODULE.bounded_counter_delta(before, after, limit=2)

    assert len(result["entries"]) == 2
    assert result["limit"] == 2
    assert result["truncated"] is True
    assert result["dropped_count"] == 4
