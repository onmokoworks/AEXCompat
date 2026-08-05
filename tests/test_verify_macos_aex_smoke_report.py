import importlib.util
import json
from pathlib import Path
import struct
import zlib

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "verify_macos_aex_smoke_report",
    ROOT / "tools" / "verify_macos_aex_smoke_report.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def png(width: int = 2, height: int = 1) -> bytes:
    def chunk(name: bytes, payload: bytes) -> bytes:
        body = name + payload
        return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))

    pixels = b"\x00" + b"\x00\x00\x00\xff" * width
    return (
        MODULE.PNG_SIGNATURE
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(pixels))
        + chunk(b"IEND", b"")
    )


def valid_report() -> dict:
    return {
        "schema_version": 1,
        "error": None,
        "gpu": {
            "requested_backend": "cpu",
            "pre_render": {"attempted": True, "completed": True, "error": 0},
            "render": {"attempted": True, "completed": True, "error": 0},
            "cleanup_complete": True,
        },
        "suite_requests": ["PF Handle Suite v2"],
        "unsupported_suite_calls": [],
        "dropped_unsupported_suite_calls": 0,
    }


def write_fixture(tmp_path: Path, report: dict | str | bytes):
    report_path = tmp_path / "diagnostic.json"
    if isinstance(report, dict):
        report_path.write_text(json.dumps(report), encoding="utf-8")
    elif isinstance(report, str):
        report_path.write_text(report, encoding="utf-8")
    else:
        report_path.write_bytes(report)
    output_path = tmp_path / "output.png"
    output_path.write_bytes(png())
    return report_path, output_path


def test_validates_successful_bounded_render_and_png(tmp_path):
    report_path, output_path = write_fixture(tmp_path, valid_report())
    result = MODULE.validate(report_path, output_path)
    assert result["status"] == "verified"
    assert (result["width"], result["height"]) == (2, 1)
    assert result["suite_request_count"] == 1


def test_accepts_classic_render_without_a_pre_render_selector(tmp_path):
    report = valid_report()
    report["render_error"] = 0
    report["gpu"]["pre_render"] = {
        "attempted": False,
        "completed": False,
    }
    report_path, output_path = write_fixture(tmp_path, report)
    assert MODULE.validate(report_path, output_path)["status"] == "verified"


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (lambda value: value.update(schema_version=2), "schema_version"),
        (lambda value: value.update(error="selector failed"), "render error"),
        (
            lambda value: value["gpu"].update(requested_backend="opencl"),
            "correctness path",
        ),
        (
            lambda value: value["gpu"]["render"].update(completed=False),
            "render did not complete",
        ),
        (lambda value: value["gpu"].update(cleanup_complete=False), "cleanup"),
        (
            lambda value: value.update(unsupported_suite_calls=[{"slot": 7}]),
            "unsupported suite",
        ),
        (
            lambda value: value.update(dropped_unsupported_suite_calls=1),
            "dropped unsupported",
        ),
    ],
)
def test_rejects_failed_or_ambiguous_diagnostics(tmp_path, mutation, message):
    report = valid_report()
    mutation(report)
    report_path, output_path = write_fixture(tmp_path, report)
    with pytest.raises(SystemExit, match=message):
        MODULE.validate(report_path, output_path)


def test_rejects_invalid_json_and_non_png_output(tmp_path):
    report_path, output_path = write_fixture(tmp_path, "not-json")
    with pytest.raises(SystemExit, match="UTF-8 JSON"):
        MODULE.validate(report_path, output_path)

    report_path.write_text(json.dumps(valid_report()), encoding="utf-8")
    output_path.write_bytes(b"not a png")
    with pytest.raises(SystemExit, match="PNG contract"):
        MODULE.validate(report_path, output_path)


def test_rejects_report_larger_than_the_trace_envelope(tmp_path):
    report_path, output_path = write_fixture(tmp_path, valid_report())
    with report_path.open("ab") as stream:
        stream.truncate(MODULE.MAX_REPORT_BYTES + 1)
    with pytest.raises(SystemExit, match="bounded contract"):
        MODULE.validate(report_path, output_path)


def test_package_scripts_pair_inputs_and_execute_the_mounted_arm64_worker():
    verifier = (ROOT / "tools/verify-macos-aex-carrier-package.sh").read_text(
        encoding="utf-8"
    )
    prepare = (ROOT / "tools/prepare-local-macos-aex-carriers.sh").read_text(
        encoding="utf-8"
    )
    for contract in (
        'smoke_aex=${2:-}',
        'smoke_input_png=${3:-}',
        '"$arm64_worker" render-trace-png',
        'verify_macos_aex_smoke_report.py',
        '"$smoke_report" "$smoke_output"',
    ):
        assert contract in verifier
    assert "x86_64/aex-guest-worker" not in verifier.split(
        'if [ -n "$smoke_aex" ]; then', 1
    )[1].split("fi", 1)[0]
    assert "AEXCOMPAT_SMOKE_AEX" in prepare
    assert "AEXCOMPAT_SMOKE_INPUT_PNG" in prepare
    assert '"$output" "$smoke_aex" "$smoke_input_png"' in prepare
