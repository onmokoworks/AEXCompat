import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_wide_time_evidence_covers_sdk_positive_and_cleanroom_negative():
    data = json.loads((ROOT / "analysis" / "PF_WIDE_TIME_INPUT_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["cases"]}
    assert data["sdk_constants"]["PF_OutFlag_WIDE_TIME_INPUT"] == 2
    assert data["sdk_constants"]["PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT"] == 131072
    allowed = cases["pf_wide_time_allowed_probe"]
    denied = cases["pf_wide_time_denied_probe"]
    sdk = cases["Adobe SDK Checkout.aex"]
    automatic = cases["pf_automatic_wide_time_allowed_probe"]
    automatic_denied = cases["pf_automatic_wide_time_denied_probe"]
    assert allowed["checkout_calls"] == allowed["checkin_calls"] == 1
    assert denied["rejected_temporal_param_checkouts"] == 1
    assert denied["checkout_calls"] == denied["checkin_calls"] == 0
    assert sdk["requested_time"] != sdk["current_time"]
    assert sdk["wide_time_checkout_allowed"] is True
    assert automatic["pre_render_error"] == automatic["smart_render_error"] == 0
    assert automatic["wide_time_checkout_allowed"] is True
    assert automatic_denied["rejected_temporal_param_checkouts"] == 1
    assert automatic_denied["wide_time_checkout_allowed"] is False
    assert all(case["param_checkouts_balanced"] for case in cases.values())


