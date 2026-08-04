import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_arbitrary_scan_requires_a_new_owned_handle():
    data = json.loads((ROOT / "analysis" / "PF_ARBITRARY_SCAN_BOUNDARY_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    colorgrid = data["adobe_sdk_colorgrid"]
    assert data["generic_contract"]["required_result"].startswith("new non-null")
    assert colorgrid["scan_callback_error"] == 0
    assert colorgrid["scan_returned_handle"] is False
    assert colorgrid["scan_failures"] == 1
    assert colorgrid["status"] == "render_completed"
    assert colorgrid["invalid_arbitrary_operations"] == 0
    assert colorgrid["handle_lifetimes_balanced"] is True
    assert data["ui_editing_enabled"] is True
    positive = data["cleanroom_positive_assignment"]
    assert positive["status"] == "render_completed"
    assert positive["scan_calls"] == 2
    assert positive["handle_lifetimes_balanced"] is True
    assert data["rejected_assignment"]["handle_lifetimes_balanced"] is True


