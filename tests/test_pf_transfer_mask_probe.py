import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-transfer-mask-probe/pf_transfer_mask_probe.cpp"
SCRIPT = ROOT / "tools/build-pf-transfer-mask-probe.ps1"
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
PROBE = ROOT / "target/pf-transfer-mask-probe-build/Release/pf_transfer_mask_probe.aex"
INPUT = ROOT / "target/gpu-effects/opencl-input.rgba"


def test_probe_builds_with_exact_mask_abi_and_flags():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    source = SOURCE.read_text(encoding="utf-8")
    assert "offsetof(PF_MaskWorld, offset) == sizeof(PF_EffectWorld)" in source
    for marker in ("PF_MaskFlag_NONE", "PF_MaskFlag_INVERTED", "PF_MaskFlag_LUMINANCE"):
        assert marker in source


def test_real_mask_probe_crosses_the_aex_boundary(tmp_path):
    output = tmp_path / "transfer-mask-output.rgba"
    completed = subprocess.run(
        [str(WORKER), "--render-image", str(PROBE),
         hashlib.sha256(PROBE.read_bytes()).hexdigest(), "v5|", str(INPUT), str(output),
         "37", "23", "0", "1", "1", "1"],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed" and report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
