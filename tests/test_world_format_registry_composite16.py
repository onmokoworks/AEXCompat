import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RENDER_SOURCE = ROOT / "minihost" / "src" / "render_subsystem.cpp"
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
WORLD_SAFETY_HEADER = ROOT / "minihost" / "src" / "worker_world_safety.hpp"
WORLD_REGISTRY_SOURCE = ROOT / "minihost" / "src" / "worker_world_registry.cpp"
WORLD_REGISTRY_HEADER = ROOT / "minihost" / "src" / "worker_world_registry.hpp"
WORLD_TRANSFORM_RUNTIME = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"
EXTERNAL_RENDER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_external_render_runtime.cpp"

def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)

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

def test_pf_world_registry_rejects_double_dispose_and_oversized_allocations():
    """Also covers PF_EffectWorld value semantics (issue #700).

    `PF_NewWorld` fills a caller-owned struct; that struct's address is not the
    world's identity. Reusing one local for a second allocation, and disposing
    through a copy of the struct, are both legal and used by real plug-ins. The
    self-test checks those alongside the rejections that must stay fail-closed.
    """
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the runtime test"
    completed = subprocess.run(
        [worker, "--self-test-pf-world-registry"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "pf_world_registry": "passed",
        "double_dispose_rejected": True,
        "world_value_semantics": True,
        "allocation_limit_rejected": True,
        "owned_snapshot_atomic": True,
        "concurrent_snapshot_dispose": True,
        "live_count": 0,
        "live_bytes": 0,
    }
