import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
WORLD_SAFETY_HEADER = ROOT / "minihost" / "src" / "worker_world_safety.hpp"


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_dispatch_registry_is_tls_scoped_and_fail_closed():
    text = WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")
    header = WORLD_SAFETY_HEADER.read_text(encoding="utf-8")
    main = SOURCE.read_text(encoding="utf-8")
    assert "thread_local std::vector<std::vector<DispatchWorldFormat>>" in text
    assert "class DispatchWorldFormatScope" in header
    assert "entry->world == world" in text
    assert "entry.data == data && entry.rowbytes == rowbytes" in text
    assert "unique->pixel_format != entry.pixel_format" in text
    assert "OwnedWorldResolution::rejected" in text
    assert "const auto exact = g_owned_worlds.find" in main
    assert "candidate.second.pixels != data" in main
    assert "OwnedWorldResolution::rejected" in main
    assert "std::abs(rowbytes) / width" not in text + main


def test_effect_world_abi_and_bounds_live_in_world_safety_component():
    header = WORLD_SAFETY_HEADER.read_text(encoding="utf-8")
    source = WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")
    assert "sizeof(LocalEffectWorld) == kEffectWorldSize" in header
    for offset in ("world_flags) == 16", "data) == 24", "rowbytes) == 32",
                   "extent_hint) == 44", "pix_aspect_ratio) == 88"):
        assert offset in header
    for bound in ("width <= 4096", "height <= 4096", "16'777'216",
                  "rowbytes >= width * pixel_bytes", "rowbytes <= 4096 * 16"):
        assert bound in source
    assert "pixel_bytes == 4 ? (flags & 1) == 0 : (flags & 1) != 0" in source


def test_classic_and_smart_dispatch_register_host_worlds_and_resizes():
    text = SOURCE.read_text(encoding="utf-8")
    assert text.count("DispatchWorldFormatScope dispatch_worlds;") >= 3
    assert text.count("register_world(output_world.data(), dispatch_pixel_format)") >= 4
    assert "register_world(map_world.world.data(), kPixelFormatArgb32)" in text
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
