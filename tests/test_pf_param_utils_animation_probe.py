import hashlib
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-param-utils-animation-probe/pf_param_utils_animation_probe.cpp"
RC = ROOT / "instruments/pf-param-utils-animation-probe/pf_param_utils_animation_probe.rc"
SCRIPT = ROOT / "tools/build-pf-param-utils-animation-probe.ps1"
PROBE = ROOT / "target/pf-param-utils-animation-probe-build/Release/pf_param_utils_animation_probe.aex"


def _worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [Path(configured) if configured else None,
                  ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
                  ROOT / "target/minihost-build-v18/aex_render_worker.exe"]
    return next((path for path in candidates if path and path.is_file()), None)


def test_probe_uses_public_sdk_abi_and_has_real_pipl():
    source = SOURCE.read_text(encoding="utf-8")
    assert '#include "AE_EffectSuites.h"' in source
    assert "PF_ParamUtilsSuite3" in source
    assert "kPFParamUtilsSuiteVersion3" in source
    assert "PF_FindKeyframeTime" in source and "PF_KeyIndexToTime" in source
    assert "PF_CheckoutKeyframe" in source and "PF_CheckinKeyframe" in source
    pipl = RC.read_text(encoding="utf-8")
    assert "16000 PiPL DISCARDABLE" in pipl and '"EffectMain' in pipl


def test_probe_builds_and_passes_native_guards():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    assert PROBE.is_file() and PROBE.stat().st_size > 0
    guard = ROOT / "tests/test_native_code_guards.py"
    subprocess.run(["pytest", "-q", str(guard)], cwd=ROOT, check=True, timeout=120)


def test_real_aex_animation_sidecar_adversarial_probe(tmp_path):
    worker = _worker()
    assert worker is not None, "build the VS2022 C++ render worker first"
    assert PROBE.is_file(), "run the focused build test first"
    input_path = tmp_path / "input.rgba"
    output_path = tmp_path / "output.rgba"
    input_path.write_bytes(bytes([255, 0, 0, 0]) * (7 * 5))
    sidecar_dir = ROOT / "target/image-transport"
    sidecar_dir.mkdir(parents=True, exist_ok=True)
    sidecar = sidecar_dir / "pf-param-utils-animation-probe.json"
    sidecar.write_text(json.dumps({"schema_version": 1, "parameters": [{"slot": 1, "keys": [
        {"time": {"value": 0, "scale": 24}, "interpolation": "linear", "value": {"type": "scalar", "value": 10}},
        {"time": {"value": 12, "scale": 24}, "interpolation": "linear", "value": {"type": "scalar", "value": 20}},
        {"time": {"value": 24, "scale": 24}, "interpolation": "hold", "value": {"type": "scalar", "value": 30}},
    ]}]}), encoding="utf-8")
    try:
        completed = subprocess.run([str(worker), "--render-image", str(PROBE),
            hashlib.sha256(PROBE.read_bytes()).hexdigest(), "v5|", str(input_path), str(output_path),
            "7", "5", "12", "1", "24", "1", "--parameter-animation-v1", str(sidecar.resolve())],
            cwd=ROOT, text=True, encoding="utf-8", errors="replace", capture_output=True,
            timeout=30, check=False)
    finally:
        sidecar.unlink(missing_ok=True)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["render_error"] == 0 and report["suite_leases_balanced"] is True
    assert report["return_message"].startswith("PFPUAP:v1 status=pass mask=0")
    assert "reject=double,foreign identical=T,F lease=released" in report["return_message"]
    assert output_path.read_bytes() == bytes([0, 211, 0, 255]) * (7 * 5)
