import json
import os
import subprocess
from pathlib import Path

from _render_session import HARNESS

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools/build-pf-param-utils-animation-probe.ps1"
PROBE = ROOT / "target/pf-param-utils-animation-probe-build/Release/pf_param_utils_animation_probe.aex"


def _worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [Path(configured) if configured else None,
                  ROOT / "target/minihost-build-v18/Release/aex_worker.exe",
                  ROOT / "target/minihost-build-v18/aex_worker.exe"]
    return next((path for path in candidates if path and path.is_file()), None)




def test_probe_builds():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    assert PROBE.is_file() and PROBE.stat().st_size > 0


def test_real_aex_animation_sidecar_adversarial_probe(tmp_path):
    worker = _worker()
    assert worker is not None, "build the VS2022 C++ render worker first"
    assert PROBE.is_file(), "run the focused build test first"
    assert HARNESS.is_file(), "build the broker Release harness first"
    input_path = tmp_path / "input.png"
    output_path = tmp_path / "output.png"
    Image.frombytes("RGBA", (7, 5), bytes([255, 0, 0, 0]) * (7 * 5)).save(input_path)
    sidecar_dir = ROOT / "target/image-transport"
    sidecar_dir.mkdir(parents=True, exist_ok=True)
    sidecar = sidecar_dir / "pf-param-utils-animation-probe.json"
    sidecar.write_text(json.dumps({"schema_version": 1, "parameters": [{"slot": 1, "keys": [
        {"time": {"value": 0, "scale": 24}, "interpolation": "linear", "value": {"type": "scalar", "value": 10}},
        {"time": {"value": 12, "scale": 24}, "interpolation": "linear", "value": {"type": "scalar", "value": 20}},
        {"time": {"value": 24, "scale": 24}, "interpolation": "hold", "value": {"type": "scalar", "value": 30}},
    ]}]}), encoding="utf-8")
    try:
        completed = subprocess.run([str(HARNESS), "--render-experimental-session-animation",
            str(PROBE), str(input_path), str(output_path), "12", "24", "1", str(sidecar.resolve())],
            cwd=ROOT, text=True, encoding="utf-8", errors="replace", capture_output=True,
            timeout=30, check=False)
    finally:
        sidecar.unlink(missing_ok=True)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["passed"] is True
    assert report["output_transport"] == "rgba8_png"
    assert report["suite_leases_balanced"] is True
    assert report["worker_diagnostics"]["exit_code"] == 0
    assert Image.open(output_path).convert("RGBA").tobytes() == bytes([0, 211, 0, 255]) * (7 * 5)
