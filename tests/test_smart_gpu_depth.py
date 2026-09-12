"""GPU-only F32 admission and no CPU selector fallback, via shipping CLI."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess

from PIL import Image
import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize("mode", ["success", "setup-fail", "pre-cpu", "setup-drop-gpu",
                                  "cpu8", "cpu16", "cpu32"])
def test_gpu_only_float_depth(tmp_path, mode):
    plugin = os.environ.get("AEXCOMPAT_TEST_GPU_DEPTH_PROBE")
    if not plugin:
        pytest.skip("requires compiled pf-smart-gpu-depth-probe and CUDA driver")
    driver = Path("C:/Windows/System32/nvcuda.dll").resolve(strict=True)
    driver_bytes = driver.read_bytes()
    # Observe the actual local module; the existing policy preflight creates
    # its own session report. No preflight report/receipt is fabricated here.
    policy = {
        "schema_version": 1,
        "expires": (datetime.datetime.now(datetime.timezone.utc)
                    + datetime.timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "modules": [{"path": "\\\\?\\" + str(driver), "basename": driver.name,
                     "size": len(driver_bytes), "backend": "cuda",
                     "sha256": hashlib.sha256(driver_bytes).hexdigest()}],
    }
    policy_path = tmp_path / "policy.json"
    policy_path.write_text(json.dumps(policy), encoding="utf-8")
    source, output = tmp_path / "input.png", tmp_path / "output.png"
    Image.new("RGBA", (32, 24), (11, 23, 37, 255)).save(source)
    args = [
        str(ROOT / "broker/target/release/aexcompat-harness.exe"), "--headless",
        "--render-experimental-smart-32-gpu-policy", plugin, str(source),
        str(output), "cuda", str(policy_path)]
    if mode.startswith("cpu"):
        args[2] = {"cpu8": "--render-experimental-smart",
                   "cpu16": "--render-experimental-smart-16",
                   "cpu32": "--render-experimental-smart-32-cpu"}[mode]
        args = args[:-2]
    run = subprocess.run(args, cwd=ROOT, capture_output=True,
        env=dict(os.environ, AEXCOMPAT_GPU_DEPTH_PROBE=mode))
    (tmp_path / "stdout.json").write_bytes(run.stdout)
    (tmp_path / "stderr.log").write_bytes(run.stderr)
    if mode == "success" or mode.startswith("cpu"):
        assert run.returncode == 0, run.stderr.decode("utf-8")
        report = json.loads(run.stdout)
        assert report["gpu_render_dispatched"] is (mode == "success")
        with Image.open(output) as image:
            image.load()
            assert image.size == (32, 24)
            pixels = image.convert("RGBA")
        for y in range(24):
            for x in range(32):
                actual = pixels.getpixel((x, y))
                expected = (int(x / 32 * 255), int(y / 24 * 255), 63)
                assert actual[3] == 255
                assert all(abs(a - b) <= 1 for a, b in zip(actual[:3], expected))
    else:
        assert run.returncode != 0 and not output.exists()
        error = run.stderr.decode("utf-8")
        report, _ = json.JSONDecoder().raw_decode(error.split("report=", 1)[1])
        assert report["smart_render_selector_dispatched"] is False
        assert report["gpu_render_dispatched"] is False
        assert report["smart_render_error"] != 0
        assert report.get("return_message") is None
        if mode == "setup-fail":
            assert report["gpu_device_setup_error"] != 0
        else:
            assert report["gpu_device_setup_error"] == 0
            assert report["gpu_render_possible"] is False
