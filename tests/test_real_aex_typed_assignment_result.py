import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_TYPED_ASSIGNMENT_RESULT_2026-07-15.json"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_particlelab_mixed_typed_assignment_reaches_classic_and_smartfx():
    result = json.loads(RESULT.read_text(encoding="utf-8"))

    assert [(item["slot"], item["kind"]) for item in result["assigned"]] == [
        (8, "integer"),
        (9, "point"),
        (11, "layer"),
        (18, "float"),
        (47, "color"),
    ]
    assert result["classic"]["passed"] is True
    assert result["classic"]["output_dimensions"] == [37, 23]
    assert result["smartfx"]["passed"] is True
    assert result["smartfx"]["output_dimensions"] == [1840, 1856]
    assert result["smartfx"]["expanded_buffer_observed"] is True
    assert result["smartfx"]["temporal_output_change_observed"] is True
    assert result["timing"] == {
        "frame": 1,
        "fps": 30,
        "duration_frames": 300,
        "current_time": 1,
        "time_step": 1,
        "total_time": 300,
        "time_scale": 30,
    }
    assert result["fractional_timebase"] == {
        "assignment_file": "tools/sdk-fixtures/particlelab-fractional-time-request.json",
        "nominal_fps": 30000 / 1001,
        "frame": 1,
        "duration_frames": 300,
        "current_time": 1001,
        "time_step": 1001,
        "total_time": 300300,
        "time_scale": 30000,
        "classic_passed": True,
        "smartfx_passed": True,
        "guard_bytes_intact": True,
        "param_checkouts_balanced": True,
        "world_lifetimes_balanced": True,
    }
    assert all(result["worker_echo"].values())
    strictness = result["strictness"]
    assert strictness["maximum_document_bytes"] == 65536
    assert strictness["maximum_assignments"] == 1024
    assert strictness["rejected_output_created"] is False
    assert strictness["timing_frame_range"] == [0, 10_000_000]
    assert strictness["timing_fps_range"] == [1, 1000]
    assert strictness["duration_frames_range"] == [1, 10_000_001]
    assert strictness["time_scale_range"] == [1, 1_000_000]
    assert strictness["time_step_range"] == [1, 100_000]
    assert all(value is True for key, value in strictness.items() if key not in {
        "maximum_document_bytes", "maximum_assignments", "rejected_output_created",
        "timing_frame_range", "timing_fps_range",
        "duration_frames_range",
        "time_scale_range", "time_step_range",
    })


def test_typed_assignment_surface_is_bounded_and_worker_observable():
    harness = HARNESS.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")

    assert "fn apply_typed_assignments(" in harness
    assert "fn typed_request_timing(" in harness
    assert "fn typed_request_document(" in harness
    assert "fn load_debug_request(&mut self)" in harness
    assert "fn save_debug_request(&mut self)" in harness
    assert 'Button::new("Load debug request...")' in harness
    assert 'Button::new("Save debug request...")' in harness
    assert '"--render-experimental-request"' in harness
    assert '"--render-experimental-smart-request"' in harness
    assert "request_bytes.len() > 64 * 1024" in harness
    assert "assignments.len() > 1024" in harness
    assert '"timing frame must be an integer within 0..=10000000"' in harness
    assert '"timing fps must be an integer within 1..=1000"' in harness
    assert "timing duration_frames must be an integer greater than frame" in harness
    assert 'ui.label("Duration frames:")' in harness
    assert 'ui.label("Time scale:")' in harness
    assert 'ui.label("Frame step:")' in harness
    assert "checked_mul(time_step)" in harness
    assert '"requested_parameters": worker_report.get("requested_parameters")' in broker
