import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"


def test_smart32_cpu_cli_is_explicit_and_bypasses_gpu_authorization(tmp_path: Path) -> None:
    source = SOURCE.read_text(encoding="utf-8")
    assert '"--render-experimental-smart-32-cpu"' in source
    assert "RenderGpuBackend::Cpu" in source

    output = tmp_path / "shifter-smart32-cpu.png"
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-smart-32-cpu", str(FIXTURE),
         str(INPUT), str(output)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report["passed"] is True
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "smartfx"
    assert report["pixel_format"] == "argb32f"
    assert report["smart_render_error"] == 0
    assert report["gpu_render_dispatched"] is False
    assert report["gpu_fallback_used"] is False
    assert report["guard_bytes_intact"] is True
    assert report["suite_leases_balanced"] is True
    assert report["handle_lifetimes_balanced"] is True
    assert report["world_lifetimes_balanced"] is True
    assert report["param_checkouts_balanced"] is True
    assert report["output_sha256"] == (
        "beab8e9e207bb415c69d5948523e47ef92634aeb7de4efa22d114a52bb8f9ef6"
    )
    assert output.is_file()
