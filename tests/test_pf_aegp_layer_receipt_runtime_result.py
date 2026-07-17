import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_AEGP_LAYER_RECEIPT_RUNTIME_RESULT_2026-07-16.json"


def test_layer_receipt_evidence_authenticates_current_artifacts():
    evidence = json.loads(RESULT.read_text(encoding="utf-8"))
    assert evidence["result"] == "ordinary_render_checked_out_real_upstream_layer_receipt"
    for artifact in evidence["authenticated_artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.is_file(), artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_layer_receipt_evidence_records_balanced_runtime_ownership():
    runtimes = json.loads(RESULT.read_text(encoding="utf-8"))["runtime"]
    assert [(run["command_mode"], run["pixel_format"]) for run in runtimes] == [
        ("--render-image", "argb8"),
        ("--render-image16", "argb16"),
        ("--render-image32", "argb32f"),
    ]
    for runtime in runtimes:
        assert runtime["exit_code"] == 0
        assert runtime["status"] == "render_completed"
        assert runtime["render_error"] == 0
        assert runtime["receipts_created"] == runtime["receipts_checked_in"] == 1
        assert runtime["live_receipts"] == 0
        assert runtime["live_receipt_bytes"] == 0
        assert runtime["invalid_receipt_operations"] == 0
        assert runtime["suite_acquires"] == runtime["suite_releases"] == 7
        assert runtime["suite_leases_balanced"] is True
        assert runtime["guard_bytes_intact"] is True
        assert runtime["last_seh_exception_code"] == 0
        assert runtime["input_sha256"] != runtime["internal_output_sha256"]
