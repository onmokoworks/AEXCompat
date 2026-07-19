import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_PATH_DATA_SUITE_RESULT_2026-07-15.json"
WORKER = ROOT / "minihost" / "src" / "worker_pf_path_runtime.hpp"
PROBE = ROOT / "instruments" / "pf-path-data-probe" / "pf_path_data_probe.cpp"


def test_all_path_data_suite1_callbacks_complete_with_lossless_image_io():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    contract = result["api_contract"]
    render = result["render"]

    assert result["result"] == "all_path_data_suite1_callbacks_completed_with_image_io"
    assert contract["suite_version"] == 1
    assert contract["callback_count"] == len(contract["callbacks_exercised"]) == 11
    assert contract["rectangle_segment_count"] == 4
    assert contract["derivative_is_unit_length"] is True
    assert render["passed"] is True
    assert render["worker_classification"] == "ok"
    assert render["dimensions"] == [37, 23]
    assert render["input_sha256"] == render["output_sha256"]
    assert render["guards_intact"] is True


def test_path_and_segment_preparation_ownership_is_balanced():
    ownership = json.loads(RESULT.read_text(encoding="utf-8"))["ownership"]

    assert ownership["path_checkout_calls"] == ownership["path_checkin_calls"] == 1
    assert ownership["segment_preps_created"] == ownership["segment_preps_disposed"] == 1
    assert ownership["suite_acquires"] == ownership["suite_releases"] == 2
    assert ownership["invalid_path_operations"] == 0
    assert ownership["path_lifetimes_balanced"] is True
    assert ownership["suite_lifetimes_balanced"] is True


def test_windows_pf_boolean_callbacks_write_exactly_one_byte():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    worker = WORKER.read_text(encoding="utf-8")
    probe = PROBE.read_text(encoding="utf-8")

    assert result["abi_fix"]["pf_boolean_windows_size_bytes"] == 1
    assert "path_is_open(void*, void*, int8_t*)" in worker
    assert "path_is_inverted(void*, int32_t, int8_t*)" in worker
    for callback in result["api_contract"]["callbacks_exercised"]:
        assert callback in probe
