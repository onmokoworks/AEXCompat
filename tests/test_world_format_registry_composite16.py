import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_dispatch_registry_is_tls_scoped_and_fail_closed():
    text = SOURCE.read_text(encoding="utf-8")
    assert "thread_local std::vector<std::vector<DispatchWorldFormat>>" in text
    assert "class DispatchWorldFormatScope" in text
    assert "entry->world == world" in text
    assert "entry.data == data && entry.rowbytes == rowbytes" in text
    assert "unique->pixel_format != entry.pixel_format" in text
    assert "const auto exact = g_owned_worlds.find" in text
    assert "candidate.second.pixels != data" in text
    assert "std::abs(rowbytes) / width" not in text


def test_classic_and_smart_dispatch_register_host_worlds_and_resizes():
    text = SOURCE.read_text(encoding="utf-8")
    assert text.count("DispatchWorldFormatScope dispatch_worlds;") >= 3
    assert text.count("register_world(output_world.data(), dispatch_pixel_format)") >= 4
    assert "register_world(map_world.data(), kPixelFormatArgb32)" in text
    assert "register_world(world.data(), dispatch_pixel_format)" in text
    assert "register_world(output_world.data(), kPixelFormatGpuBgra128)" in text


def test_composite16_runtime_provenance_and_concurrency_matrix():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the runtime test"
    completed = subprocess.run(
        [worker, "--self-test-world-transform-composite"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {"world_transform_composite_rect": "passed"}
