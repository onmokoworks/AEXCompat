import os
from pathlib import Path

import hashlib
import json
import subprocess


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-aegp-async-cancel-probe/pf_aegp_async_cancel_probe.cpp"
HOST = ROOT / "minihost/src/l2_main.cpp"


def test_fixture_cancels_immediately_and_requires_one_canceled_callback_without_receipt():
    source = SOURCE.read_text(encoding="utf-8")
    submit = source.index("AEGP_RenderAndCheckoutLayerFrame_Async(")
    cancel = source.index("AEGP_CancelAsyncRequest(request_id)", submit)
    wait = source.index("result.ready.wait_for", cancel)
    assert submit < cancel < wait
    assert "callback_count.load(std::memory_order_relaxed) != 1" in source
    assert "!result.canceled" in source
    assert "result.error != A_Err_NONE" in source
    assert "result.receipt != nullptr" in source
    assert "AEGP_CheckinFrame" not in source


def test_current_host_needs_a_pre_claim_gate_for_deterministic_cancellation():
    host = HOST.read_text(encoding="utf-8")
    assert 'AEXCOMPAT_TEST_ASYNC_CANCEL_GATE' in host
    assert "request->gate_changed.wait_for" in host
    assert "request->state.compare_exchange_strong(expected, 1)" in host
    assert "found->second->state.compare_exchange_strong(expected, 2)" in host
    assert "found->second->gate_changed.notify_one()" in host


def test_real_probe_deterministically_cancels_before_completion(tmp_path):
    worker = ROOT / "target/minihost-build/aex_render_worker.exe"
    probe = ROOT / "target/pf-aegp-async-cancel-probe-build/Release/pf_aegp_async_cancel_probe.aex"
    source = ROOT / "target/gpu-effects/opencl-input.rgba"
    output = tmp_path / "async-canceled-output.rgba"
    env = os.environ.copy()
    env["AEXCOMPAT_TEST_ASYNC_CANCEL_GATE"] = "1"
    completed = subprocess.run(
        [str(worker), "--render-image", str(probe),
         hashlib.sha256(probe.read_bytes()).hexdigest(), "v5|", str(source),
         str(output), "37", "23", "0", "1", "1", "1"],
        cwd=ROOT, env=env, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=30,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed"
    assert report["async_layer_requests_balanced"] is True
    assert report["async_layer_requests_created"] == 1
    assert report["async_layer_requests_completed"] == 0
    assert report["async_layer_requests_canceled"] == 1
    assert report["async_layer_callback_failures"] == 0
    assert report["async_layer_callback_exceptions"] == 0
    assert report["live_async_layer_requests"] == 0
    assert report["async_layer_reserved_bytes"] == 0
    assert report["receipts_created"] == 0
    assert report["receipts_checked_in"] == 0
    assert report["live_receipts"] == 0
