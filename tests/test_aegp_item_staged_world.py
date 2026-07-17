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
        ROOT / "target" / "minihost-timed-layers" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_item_checkout_uses_immutable_host_stage_not_reentrant_render():
    text = SOURCE.read_text(encoding="utf-8")
    for marker in (
        "struct StagedItemWorld",
        "publish_staged_item_world(",
        "snapshot_staged_item_world(",
        "transform_staged_item_world(",
        "register_item_receipt(",
        "staged_source_pin",
        "thread_local std::vector<ItemRenderStackKey> g_item_render_stack",
        "snapshot_staged_item_world(snapshot, stage)",
        "publish_staged_item_world(aegp_comp_item_handle()",
    ):
        assert marker in text
    assert "render_loaded_effect_item_receipt(" not in text
    assert "g_loaded_effect_receipt_mutex" not in text
    assert "g_loaded_effect_receipt_active" not in text


def test_stage_key_covers_render_identity_and_generation():
    text = SOURCE.read_text(encoding="utf-8")
    for marker in (
        "value.item == options.item",
        "same_stage_rational(value.time, options.time)",
        "same_stage_rational(value.time_step, options.time_step)",
        "value.quality == options.render_quality",
        "value.guide_layers == options.render_guide_layers",
        "value.pixel_format == pixel_format",
        "value.project_generation == generation",
        "value.width == width",
        "value.height == height",
        "value.rowbytes == tight_rowbytes",
        "stage_generation",
    ):
        assert marker in text


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
    report = json.loads(completed.stdout)
    assert report["aegp_item_staged_worlds"] == "passed"
    assert report["immutable_stage"] is True
    assert report["reentrant_render_used"] is False
    assert report["published"] >= 4
    assert report["cache_hits"] >= 9
    assert report["cache_misses"] >= 4
    assert report["cycles_rejected"] >= 1
