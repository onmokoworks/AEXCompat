import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GAMMA_PERSISTENT_SEQUENCE_RESULT_2026-07-15.json"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"

def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))

def test_two_frames_share_exactly_one_sequence_lifecycle():
    sequence = result()["sequence"]
    assert sequence["persistent"] is True
    assert sequence["sequence_setup_calls"] == sequence["sequence_setdown_calls"] == 1
    assert sequence["frame_setup_calls"] == sequence["frame_setdown_calls"] == 2
    assert sequence["sequence_setup_error"] == sequence["sequence_setdown_error"] == 0
    assert sequence["frame_errors"] == [0, 0]
    assert sequence["frame_times"] == [0, 1]

def test_gamma_sequence_handle_survives_both_frames_and_is_disposed_once():
    evidence = result()
    ownership = evidence["ownership"]
    render = evidence["render"]
    assert ownership["sequence_handles_created"] == 1
    assert ownership["sequence_handles_disposed"] == 1
    assert ownership["handle_lifetimes_balanced"] is True
    assert ownership["world_lifetimes_balanced"] is True
    assert ownership["suite_leases_balanced"] is True
    assert ownership["param_checkouts_balanced"] is True
    assert render["status"] == "render_completed"
    assert render["guard_bytes_intact"] is True
    assert len(render["frame_hashes"]) == 2

