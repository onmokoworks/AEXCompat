import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_path_parameter_assignment_resolves_only_connected_masks():
    data = json.loads((ROOT / "analysis" / "PF_PATH_PARAMETER_ASSIGNMENT_RESULT_2026-07-15.json").read_text())
    assert data["sdk_semantics"]["none"] == 0
    assert data["positive"]["requested_index"] == data["positive"]["observed_path_id"] == 2
    assert data["positive"]["status"] == "render_completed"
    assert data["positive"]["guard_bytes_intact"] is True
    assert data["negative"]["worker_exit_code"] == 3
    assert data["negative"]["render_selector_dispatched"] is False
    assert data["negative"]["output_created"] is False
    defaults = {case["descriptor_default"]: case for case in data["default_resolution"]}
    assert defaults[0]["observed_path_id"] == 0
    assert defaults[0]["implicit_first_mask_selected"] is False
    assert defaults[2]["observed_path_id"] == 2
    assert defaults[3]["missing_default_resolves_none"] is True
    assert data["assignment_precedence"]["explicit_assignment_wins"] is True


def test_path_assignment_is_slot_bound_across_ui_broker_and_worker():
    worker = "\n".join((ROOT / "minihost" / "src" / name).read_text() for name in (
        "l2_main.cpp", "worker_parameter_execution.cpp"))
    broker = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text()
    harness = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text()
    assert "descriptor.type == 7 || descriptor.type == 12" in worker
    assert "hooks().active_mask_count()" in worker
    assert "runtime().records[i].default_value" in worker
    assert '"integer" | "path" if item.value.fract() == 0.0' in broker
    assert '"integer" | "float" | "path"' in harness
    assert 'parameter.kind == "path"' in harness
