import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_MULTI_LAYER_INPUT_RESULT_2026-07-15.json"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_real_aex_classic_and_smartfx_accept_multiple_slot_bound_layers():
    result = json.loads(RESULT.read_text(encoding="utf-8"))

    for fixture, slots in (("refraction_dispersion", [26, 27]), ("particle_lab", [11, 93, 103, 141])):
        observation = result[fixture]
        assert observation["layer_slots"] == slots
        assert observation["layer_count"] == len(slots)
        assert observation["dimensions"] == [37, 23]
        assert observation["classic_passed"] is True
        assert observation["smartfx_passed"] is True
        assert observation["guard_bytes_intact"] is True
        assert observation["param_checkouts_balanced"] is True

    negative = result["negative_cases"]
    assert negative["duplicate_slot_rejected"] is True
    assert negative["non_layer_slot_rejected"] is True
    assert negative["rejected_output_created"] is False
    assert negative["assignments_applied_atomically"] is True
    assert result["transport_limits"]["maximum_secondary_layers"] == 8


def test_layer_cli_and_broker_keep_slot_binding_fail_closed():
    harness = HARNESS.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")

    assert '"--render-experimental-layer-slots"' in harness
    assert '"--render-experimental-smart-layer-slots"' in harness
    assert "fn assign_layer_paths(" in harness
    assert "let mut planned = Vec::new();" in harness
    assert "if selected_layers.len() > 8" in broker
    assert "secondary layer slots must be unique" in broker
