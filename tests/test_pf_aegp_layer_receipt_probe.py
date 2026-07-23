import hashlib
import json
import struct
import subprocess
from pathlib import Path

from _render_session import run_session_render

import pytest


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-layer-receipt-probe" / "pf_aegp_layer_receipt_probe.cpp"
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-layer-receipt-probe-build" / "Release" / "pf_aegp_layer_receipt_probe.aex"
INPUT = ROOT / "target" / "gpu-effects" / "opencl-input.rgba"


def test_probe_uses_the_requested_aegp_receipt_path():
    source = SOURCE.read_text(encoding="utf-8")
    for token in (
        "kAEGPPFInterfaceSuiteVersion1",
        "kAEGPEffectSuiteVersion3",
        "kAEGPLayerRenderOptionsSuiteVersion1",
        "kAEGPRenderSuiteVersion5",
        "kAEGPWorldSuiteVersion3",
        "AEGP_GetNewEffectForEffect",
        "AEGP_NewFromUpstreamOfEffect",
        "AEGP_RenderAndCheckoutLayerFrame",
        "AEGP_GetReceiptWorld",
        "AEGP_GetBaseAddr8",
        "AEGP_GetBaseAddr16",
        "AEGP_GetBaseAddr32",
        "PF_GetPixelFormat",
        "PF_PixelFormat_ARGB32",
        "PF_PixelFormat_ARGB64",
        "PF_PixelFormat_ARGB128",
    ):
        assert token in source


def test_probe_unwinds_all_owned_resources():
    source = SOURCE.read_text(encoding="utf-8")
    assert "AEGP_CheckinFrame(receipt)" in source
    assert "AEGP_Dispose(options)" in source
    assert "AEGP_DisposeEffect(effect)" in source
    assert source.count("ReleaseSuite(") >= 7
    assert "std::memset(output->data" in source
    assert "rowbytes < static_cast<A_u_long>(width) * pixel_size" in source


def _float32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _expected_output(source, width, depth):
    # The worker writes the output world at its native depth (rgba16le /
    # float32le), so mirror the promotion it applies to the checked-out
    # upstream world (rgba8_to_argb) and the probe's per-depth transform.
    expected = bytearray()
    for pixel in range(width * (len(source) // (width * 4))):
        x, y = pixel % width, pixel // width
        red, green, blue, alpha = source[pixel * 4:pixel * 4 + 4]
        if depth == 8:
            expected += bytes((green ^ (x & 0xff), blue ^ (y & 0xff),
                               red ^ ((x + y) & 0xff), alpha))
        elif depth == 16:
            to_16 = lambda value: (value * 32768 + 127) // 255
            expected += struct.pack(
                "<4H",
                to_16(green) ^ (x & 0xffff),
                to_16(blue) ^ (y & 0xffff),
                to_16(red) ^ ((x + y) & 0xffff),
                to_16(alpha),
            )
        else:
            to_float = lambda value: _float32(value / 255.0)
            expected += struct.pack(
                "<4f",
                _float32(to_float(green) + x / 65536.0),
                _float32(to_float(blue) + y / 65536.0),
                _float32(to_float(red) + (x + y) / 65536.0),
                to_float(alpha),
            )
    return expected


@pytest.mark.parametrize(
    ("pixel_format", "depth"),
    (("argb8", 8), ("argb16", 16), ("argb32f", 32)),
)
def test_real_probe_checks_out_typed_upstream_pixels_during_ordinary_render(
        tmp_path, pixel_format, depth):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert INPUT.is_file()
    output = tmp_path / f"layer-receipt-output-{depth}.rgba"
    probe_hash = hashlib.sha256(PROBE.read_bytes()).hexdigest()
    report = run_session_render(
        tmp_path, PROBE, INPUT, output, width=37, height=23,
        pixel_format=pixel_format,
    )
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["receipt_lifetimes_balanced"] is True
    assert report["receipts_created"] == 1
    assert report["receipts_checked_in"] == 1
    assert report["live_receipts"] == 0
    assert report["live_receipt_bytes"] == 0
    assert report["invalid_receipt_operations"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 7
    assert report["guard_bytes_intact"] is True

    source = INPUT.read_bytes()
    assert output.read_bytes() == _expected_output(source, 37, depth)
    assert report["input_sha256"] != report["output_sha256"]
