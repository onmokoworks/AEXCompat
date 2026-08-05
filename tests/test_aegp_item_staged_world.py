import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
HEADER = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.hpp"
ENTRY_WIRING = ROOT / "minihost" / "src" / "worker_entry_wiring.cpp"
RECEIPTS_HEADER = ROOT / "minihost" / "src" / "worker_render_receipts.hpp"
RECEIPTS = ROOT / "minihost" / "src" / "worker_render_receipts.cpp"
LAYER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_layer_render_runtime.cpp"
LAYER_HEADER = ROOT / "minihost" / "src" / "worker_aegp_layer_render_runtime.hpp"

def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-timed-layers" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)

def test_native_item_stage_pixel_oracle():
    worker = _worker()
    assert worker is not None, "build the production render worker first"
    completed = subprocess.run(
        [str(worker), "--self-test-aegp-item-staged-worlds"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    def reject_duplicate_keys(pairs):
        result = {}
        for key, value in pairs:
            assert key not in result, f"duplicate JSON key: {key}"
            result[key] = value
        return result

    report = json.loads(completed.stdout, object_pairs_hook=reject_duplicate_keys)
    assert report["aegp_item_staged_worlds"] == "passed"
    assert report["immutable_stage"] is True
    assert report["reentrant_render_used"] is False
    assert report["published"] >= 4
    assert report["cache_hits"] >= 9
    assert report["cache_misses"] >= 4
    assert report["cycles_rejected"] >= 1
    assert report["exact_hits"] > 0
    assert report["hold_hits"] > 0
    assert report["nearest_hits"] > 0
    assert report["direct_cycles_rejected"] > 0
    assert report["indirect_cycles_rejected"] > 0
    assert report["depth_limit_rejections"] > 0
    assert report["stage_limit_rejections"] > 0
    assert report["effect_boundary_rejections"] > 0
    assert report["partial_failures"] > 0
    assert report["cleanup_count"] > 0
    assert report["in_flight"] == 0
    assert report["max_in_flight"] >= 2
    assert report["registered_items"] == 0
    assert report["cached_stages"] == 0
    assert report["cached_bytes"] == 0
    assert report["last_trace_hash"] != 0
    assert report["last_stage_identity_hash"] != 0
    assert report["last_resolved_stages"] == 8
    assert report["max_resolved_depth"] >= 2
