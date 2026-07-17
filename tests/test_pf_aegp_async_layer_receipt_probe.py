import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-async-layer-receipt-probe" / "pf_aegp_async_layer_receipt_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-async-layer-receipt-probe-build" / "Release" / "pf_aegp_async_layer_receipt_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_has_bounded_validated_async_slot2_path():
    source = SOURCE.read_text(encoding="utf-8")
    for token in (
        "AEGP_RenderAndCheckoutLayerFrame_Async", "Render Suite5 slot 2",
        "std::chrono::seconds(5)", "callback_count", "callback_request_id != request_id",
        "refcon == result->expected_refcon", "!callback_state->refcon_matches",
        "AEGP_CancelAsyncRequest", "AEGP_WorldType_8", "AEGP_GetBaseAddr8",
        "AEGP_CheckinFrame(receipt)", "AEGP_Dispose(options)", "AEGP_DisposeEffect(effect)",
    ):
        assert token in source


def test_real_probe_uses_async_owned_receipt_during_ordinary_render(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert INPUT.is_file()
    output = tmp_path / "async-layer-receipt-output.rgba"
    result = subprocess.run(
        [str(WORKER), "--render-image", str(PROBE), hashlib.sha256(PROBE.read_bytes()).hexdigest(),
         "v5|", str(INPUT), str(output), "37", "23", "0", "1", "1", "1"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=30,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["async_layer_requests_balanced"] is True
    assert report["async_layer_requests_created"] == 1
    assert report["async_layer_requests_completed"] == 1
    assert report["async_layer_requests_canceled"] == 0
    assert report["live_async_layer_requests"] == 0
    assert report["async_layer_reserved_bytes"] == 0
    assert report["receipt_lifetimes_balanced"] is True
    assert report["receipts_created"] == report["receipts_checked_in"] == 1
    assert report["live_receipts"] == report["live_receipt_bytes"] == 0
    assert report["invalid_receipt_operations"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 7
    assert report["guard_bytes_intact"] is True

    source = INPUT.read_bytes()
    expected = bytearray(len(source))
    for pixel in range(37 * 23):
        x, y = pixel % 37, pixel // 37
        red, green, blue, alpha = source[pixel * 4:pixel * 4 + 4]
        expected[pixel * 4:pixel * 4 + 4] = bytes(
            (green ^ (x & 0xff), blue ^ (y & 0xff), red ^ ((x + y) & 0xff), alpha))
    assert output.read_bytes() == expected
    assert report["input_sha256"] != report["output_sha256"]
