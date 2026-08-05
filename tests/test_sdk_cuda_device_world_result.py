import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_cuda_device_world_matches_cpu_public_pixels_and_balances_ownership():
    evidence = json.loads(
        (ROOT / "analysis" / "SDK_CUDA_DEVICE_WORLD_RESULT_2026-07-15.json").read_text(encoding="utf-8")
    )
    assert evidence["fixture"]["source_modified"] is False
    assert evidence["one_pixel_oracle"]["output_rgba8"] == [245, 235, 225, 255]
    image = evidence["image_oracle"]
    assert image["gpu_selector_error"] == image["gpu_render_error"] == 0
    assert image["gpu_cpu_different_rgba8_bytes"] == 0
    assert image["gpu_cpu_max_rgba8_error"] == 0
    ownership = evidence["ownership"]
    assert ownership["device_allocations_created"] == ownership["device_allocations_freed"] == 3
    assert ownership["live_device_allocations"] == ownership["live_device_bytes"] == 0
    assert ownership["cuda_sync_failures"] == 0
    assert ownership["gpu_memory_lifetimes_balanced"] is True
    assert ownership["guard_bytes_intact"] is True
    assert evidence["broker"]["gpu_fallback_used"] is False


