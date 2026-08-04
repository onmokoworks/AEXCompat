import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "SDK_GPU_NEGOTIATION_BOUNDARY_RESULT_2026-07-15.json"
SOURCE = source_owners.L2_SOURCE
SMART_DISPATCH = ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp"
SMART_FINALIZE = ROOT / "minihost" / "src" / "worker_smart_finalize.cpp"
TRANSPORT = ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_gpu_negotiation_rejects_unwritten_output_without_setdown_fault():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    observation = evidence["observation"]
    assert observation["gpu_device_setup_error"] == 0
    assert observation["smart_pre_render_error"] == 0
    assert observation["gpu_render_possible"] is True
    assert observation["gpu_render_dispatched"] is True
    assert observation["smart_render_selector_error"] == 0
    assert observation["smart_render_error"] == -6
    assert observation["output_pixels_valid"] is False
    assert observation["gpu_device_setdown_error"] == 0
    assert observation["gpu_device_setdown_exception_code"] == 0
    assert observation["failure_stage"] == "output_validation"
    assert observation["output_created"] is False
    fallback = evidence["broker_fallback"]
    assert fallback["fresh_cpu_worker"] is True
    assert fallback["gpu_attempt_failure_stage"] == "output_validation"
    assert fallback["cpu_fallback_passed"] is True
    assert fallback["output_created"] is True




