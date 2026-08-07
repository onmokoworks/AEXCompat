from pathlib import Path

from _render_session import run_session_render


ROOT = Path(__file__).resolve().parents[1]






def test_real_probe_deterministically_cancels_before_completion(tmp_path, monkeypatch):
    probe = ROOT / "target/pf-aegp-async-cancel-probe-build/Release/pf_aegp_async_cancel_probe.aex"
    source = ROOT / "target/gpu-effects/opencl-input.rgba"
    output = tmp_path / "async-canceled-output.rgba"
    monkeypatch.setenv("AEXCOMPAT_TEST_ASYNC_CANCEL_GATE", "1")
    report = run_session_render(
        tmp_path,
        probe,
        source,
        output,
        width=37,
        height=23,
    )
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
