"""ARGB8 Auto GPU admission through the shipping Smart render CLI."""

import hashlib
import json
import os
from pathlib import Path
import subprocess

from PIL import Image
import pytest


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker/target/release/aexcompat-harness.exe"
WORKER = ROOT / "target/minihost-build/aex_worker.exe"
WIDTH = 32
HEIGHT = 24
CPU_DIRECT = (231, 17, 29, 255)
CPU_AFTER_GPU_ATTEMPT = (19, 43, 227, 255)
FAILURE_FIELDS = {
    "dual8-setup-fail": ("setup_error", False),
    "dual8-cleanup-error": ("cleanup_error", True),
    "dual8-pre-error": ("pre_error", False),
    "dual8-pre-unbalanced": ("render_error", False),
    "dual8-invalid-rect": ("render_error", False),
    "dual8-gpu-render-error": ("render_error", True),
    "dual8-gpu-setdown-error": ("setdown_error", True),
    "dual8-frame-setdown-error": ("lifecycle_error", True),
}


def _probe() -> Path:
    value = os.environ.get("AEXCOMPAT_TEST_GPU_DEPTH_PROBE")
    if not value:
        pytest.skip("requires compiled pf-smart-gpu-depth-probe")
    probe = Path(value).resolve(strict=True)
    assert HARNESS.is_file(), f"missing Release harness: {HARNESS}"
    assert WORKER.is_file(), f"missing Release worker: {WORKER}"
    return probe


def _failure_report(stderr: bytes) -> dict:
    text = stderr.decode("utf-8", errors="replace")
    assert "report=" in text, text
    report, _ = json.JSONDecoder().raw_decode(text.split("report=", 1)[1])
    return report


def _gpu_attempt(report: dict) -> dict:
    for container in (report, report.get("final_report", {})):
        for key in ("gpu_attempt", "gpu_auto8_attempt"):
            attempt = container.get(key) if isinstance(container, dict) else None
            if isinstance(attempt, dict):
                return attempt
    raise AssertionError(f"failure report has no GPU attempt evidence: {report}")


def _argb_sha256(path: Path) -> str:
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
        rgba_bytes = rgba.tobytes()
        argb = bytearray()
        for offset in range(0, len(rgba_bytes), 4):
            red, green, blue, alpha = rgba_bytes[offset : offset + 4]
            argb.extend((alpha, red, green, blue))
    return hashlib.sha256(argb).hexdigest()


def _assert_solid(path: Path, expected: tuple[int, int, int, int]) -> None:
    with Image.open(path) as image:
        image.load()
        assert image.size == (WIDTH, HEIGHT)
        pixels = image.convert("RGBA")
    assert pixels.tobytes() == bytes(expected) * (WIDTH * HEIGHT)


def _assert_gpu_pattern(path: Path) -> None:
    with Image.open(path) as image:
        image.load()
        assert image.size == (WIDTH, HEIGHT)
        pixels = image.convert("RGBA")
    for y in range(HEIGHT):
        for x in range(WIDTH):
            actual = pixels.getpixel((x, y))
            expected = (int(x / WIDTH * 255), int(y / HEIGHT * 255), 63)
            assert actual[3] == 255
            assert all(abs(value - target) <= 1 for value, target in zip(actual[:3], expected))


@pytest.mark.parametrize(
    ("mode", "expected"),
    [
        ("dual8-capable", "gpu"),
        ("dual8-no-cap", "cpu-direct"),
        ("dual8-pre-cpu", "cpu-after-gpu-attempt"),
        ("dual8-setup-fail", "failure"),
        ("dual8-cleanup-error", "failure"),
        ("dual8-pre-error", "failure"),
        ("dual8-pre-unbalanced", "failure"),
        ("dual8-invalid-rect", "failure"),
        ("dual8-gpu-render-error", "failure"),
        ("dual8-gpu-setdown-error", "failure"),
        ("dual8-frame-setdown-error", "failure"),
    ],
)
def test_argb8_auto_obeys_gpu_f32_capability_and_refusals(tmp_path, mode, expected):
    probe = _probe()
    if expected != "cpu-direct" and not Path("C:/Windows/System32/nvcuda.dll").is_file():
        pytest.skip("this Auto branch requires a CUDA driver")

    source = tmp_path / f"source-{mode}.png"
    output = tmp_path / f"output-{mode}.png"
    Image.new("RGBA", (WIDTH, HEIGHT), (11, 23, 37, 255)).save(source)
    environment = dict(os.environ, AEXCOMPAT_GPU_DEPTH_PROBE=mode)
    environment.pop("AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS", None)
    run = subprocess.run(
        [
            str(HARNESS),
            "--headless",
            "--render-experimental-smart",
            str(probe),
            str(source),
            str(output),
        ],
        cwd=ROOT,
        env=environment,
        capture_output=True,
    )
    (tmp_path / f"stdout-{mode}.json").write_bytes(run.stdout)
    (tmp_path / f"stderr-{mode}.log").write_bytes(run.stderr)

    if expected == "failure":
        assert run.returncode != 0
        assert not output.exists()
        if mode == "dual8-pre-unbalanced":
            # The shipping wrapper deliberately withholds the rejected close's
            # final report. This proves fail-closed/no-output only; the raw
            # RenderSession test owns the selector-count/no-CPU-retry oracle.
            error = run.stderr.decode("utf-8", errors="replace")
            assert run.stdout == b""
            assert (
                "render session close rejected invariant=worker_not_ok "
                "invalidated=false worker_ok=false"
            ) in error
            assert "there is no alternate transport" in error
            return
        report = _failure_report(run.stderr)
        attempt = _gpu_attempt(report)
        failure_field, render_dispatched = FAILURE_FIELDS[mode]
        assert attempt["fallback_used"] is False
        assert attempt[failure_field] != 0
        assert attempt["render_dispatched"] is render_dispatched
        return

    assert run.returncode == 0, run.stderr.decode("utf-8", errors="replace")
    report = json.loads(run.stdout)
    assert report["passed"] is True
    assert report["pixel_format"] == "argb8"
    assert report["output_transport"] == "rgba8_png"
    assert report["output_pixels_valid"] is True
    assert report["smart_render_selector_dispatched"] is True
    assert report["input_sha256"] == _argb_sha256(source)
    assert report["output_sha256"] == _argb_sha256(output)

    if expected == "gpu":
        attempt = report["gpu_attempt"]
        assert attempt["fallback_used"] is False
        assert attempt["setup_dispatched"] is True
        assert attempt["render_dispatched"] is True
        assert len(attempt["internal_float_input_sha256"]) == 64
        assert len(attempt["internal_float_output_sha256"]) == 64
        assert report["gpu_device_setup_error"] == 0
        assert report["gpu_render_possible"] is True
        assert report["gpu_render_dispatched"] is True
        _assert_gpu_pattern(output)
    elif expected == "cpu-direct":
        assert report["gpu_render_possible"] is False
        assert report["gpu_render_dispatched"] is False
        _assert_solid(output, CPU_DIRECT)
    else:
        assert expected == "cpu-after-gpu-attempt"
        attempt = report["gpu_attempt"]
        assert attempt["fallback_used"] is True
        assert attempt["fallback_reason"] == "pre-render-declined-gpu"
        assert attempt["setup_dispatched"] is True
        assert attempt["render_dispatched"] is False
        assert report["gpu_render_possible"] is False
        assert report["gpu_render_dispatched"] is False
        _assert_solid(output, CPU_AFTER_GPU_ATTEMPT)
