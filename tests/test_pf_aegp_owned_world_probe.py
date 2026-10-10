import subprocess
from pathlib import Path
from PIL import Image

from _render_session import HARNESS, assert_artifact_fresh, run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-aegp-owned-world-probe" / "pf_aegp_owned_world_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-aegp-owned-world-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE = ROOT / "target" / "pf-aegp-owned-world-probe-build" / "Release" / "pf_aegp_owned_world_probe.aex"


def test_probe_builds_against_world_suite3():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT, check=True, timeout=180,
    )
    assert PROBE.is_file()


def test_real_probe_exercises_owned_world_lifecycle(tmp_path):
    assert WORKER.is_file()
    assert PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    source_image = tmp_path / "owned-world-input.png"
    Image.new("RGBA", (37, 23), (17, 44, 91, 255)).save(source_image)
    output = tmp_path / "owned-world-output.rgba"
    report = run_session_render(tmp_path, PROBE, source_image, output, width=37, height=23)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert report["suite_acquires"] == report["suite_releases"] == 3
    assert report["guard_bytes_intact"] is True
    pixels = output.read_bytes()
    assert pixels == bytes((32, 128, 255, 255)) * (37 * 23)
    decoded = Image.frombytes("RGBA", (37, 23), pixels)
    assert decoded.getbbox() == (0, 0, 37, 23)
    assert decoded.getpixel((36, 22)) == (32, 128, 255, 255)
