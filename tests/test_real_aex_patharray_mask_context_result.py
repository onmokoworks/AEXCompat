import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_PATHARRAY_MASK_CONTEXT_RESULT_2026-07-15.json"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"


def test_patharray_mask_context_turns_smartfx_passthrough_into_effect_output():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    assert result["without_mask_context"]["smartfx_differing_input_pixels"] == 0
    connected = result["with_mask_context"]
    assert connected["classic_passed"] is True
    assert connected["smartfx_passed"] is True
    assert connected["classic_differing_input_pixels"] == 37 * 23
    assert connected["smartfx_differing_input_pixels"] == 37 * 23
    assert all(result["invariants"].values())


def test_generic_mask_transport_reuses_bounded_cleanroom_context():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    limits = result["transport_limits"]
    assert limits["maximum_masks"] == 8
    assert limits["maximum_vertices_per_mask"] == 64
    assert limits["maximum_total_vertices"] == 128
    assert limits["maximum_encoded_bytes"] == 8192
    assert result["negative_control"]["closed_mask_with_two_vertices_rejected"] is True
    assert result["negative_control"]["output_created"] is False
    assert result["negative_control"]["native_worker_started"] is False
    harness = HARNESS.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    worker = WORKER.read_text(encoding="utf-8")
    assert "fn typed_request_host_context(" in harness
    assert "render_experimental_image_at_time_with_format_and_context" in harness
    assert "host_context: Option<&crate::render_request::HostContext>" in broker
    assert "encode_mask_context(context)?" in broker
    assert "image_mask_context" in worker
    assert "parse_mask_context_payload(argv[image_trailer_argc - 1])" in worker
    assert "parse_mask_context_payload(argv[smart_image_trailer_argc - 1])" in worker
