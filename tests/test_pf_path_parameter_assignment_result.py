import json
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]


def test_path_parameter_assignment_resolves_only_connected_masks():
    data = json.loads((ROOT / "analysis" / "PF_PATH_PARAMETER_ASSIGNMENT_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
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


