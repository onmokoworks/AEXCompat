import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_AEGP_ASYNC_CANCEL_RUNTIME_RESULT_2026-07-16.json"


def _load():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_async_cancel_evidence_authenticates_current_artifacts():
    for artifact in _load()["authenticated_artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.is_file(), artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_async_cancel_evidence_records_balanced_canceled_callback():
    runtime = _load()["runtime"]
    assert runtime["test_environment"] == "AEXCOMPAT_TEST_ASYNC_CANCEL_GATE=1"
    assert runtime["exit_code"] == 0
    assert runtime["status"] == "render_completed"
    assert runtime["async_layer_requests_created"] == 1
    assert runtime["async_layer_requests_completed"] == 0
    assert runtime["async_layer_requests_canceled"] == 1
    assert runtime["async_layer_callback_failures"] == 0
    assert runtime["async_layer_callback_exceptions"] == 0
    assert runtime["live_async_layer_requests"] == 0
    assert runtime["async_layer_reserved_bytes"] == 0
    assert runtime["receipts_created"] == runtime["receipts_checked_in"] == 0
    assert runtime["live_receipts"] == 0
    assert runtime["receipt_lifetimes_balanced"] is True
    assert runtime["suite_acquires"] == runtime["suite_releases"] == 5
    assert runtime["guard_bytes_intact"] is True
    assert runtime["last_seh_exception_code"] == 0
    assert runtime["internal_output_sha256"] == runtime["external_output_sha256"]
