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


def test_gpu_abi_and_suite_table_are_explicit():
    source = "".join(
        path.read_text(encoding="utf-8")
        for path in (SOURCE, source_owners.SRC / "worker_host_suite_wiring.cpp", SMART_DISPATCH, SMART_FINALIZE)
    )
    for marker in (
        "write<int16_t>(pre_input, 44",
        "write<void*>(pre_input, 48",
        "write<int32_t>(pre_input, 56",
        "write<uint32_t>(pre_input, 60",
        "write<void*>(smart_input, 56",
        "write<int32_t>(smart_input, 64",
        "write<uint32_t>(smart_input, 68",
        "write<int32_t>(setdown_input, 8, gpu_framework)",
        '{"PF GPU Device Suite", 1, g_gpu_device_suite1.data()}',
        "kPixelFormatGpuBgra128",
        # Empty legal results are exempt; every rendered output still passes
        # the untouched/finite validation.
        "result.output_pixels_valid = result.empty_result_rect",
        "!logical_output.empty() && !untouched && finite",
    ):
        assert marker in source
    transport = TRANSPORT.read_text(encoding="utf-8")
    assert "std::array<void*, 15> gpu_device_suite1" in transport
    assert "write<int32_t>(setdown_input, 8, 4)" not in source


def test_gpu_cleanup_error_is_a_hard_failure_and_forwarded():
    source = SOURCE.read_text(encoding="utf-8") + SMART_FINALIZE.read_text(encoding="utf-8")
    broker = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    assert "invoke_entry_seh" in source
    assert "smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0" in source
    assert "result.render_error == 0 && !result.output_pixels_valid" in source
    assert 'worker_report.get("gpu_device_setdown_error") == Some(&json!(0))' in broker
    assert 'diagnostics["failure_stage"] = json!("output_validation")' in broker
    assert '"gpu_device_setdown_exception_code": worker_report.get(' in broker
    # The two `initial_report.as_ref()` projections copied the failed GPU
    # launch's fields into the `gpu_attempt` record before the CPU retry. #365
    # deleted that retry with the one-shot transport, so those fields reach the
    # public report through the ordinary flattening and the failure is an error
    # rather than an attempt record.
    assert "initial_report" not in broker
    assert '"smart_render_selector_error",' in broker
    assert '"output_pixels_valid",' in broker
