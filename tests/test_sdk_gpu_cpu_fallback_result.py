import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sdk_gpu_failure_uses_a_fresh_cpu_worker_and_rejects_gpu_output():
    result = json.loads(
        (ROOT / "analysis" / "SDK_GPU_CPU_FALLBACK_RESULT_2026-07-15.json").read_text()
    )
    render = result["render"]
    assert result["fixture_sha256"] == "f782038561c0c62c018abada0f952a3cbbd21bbbcf57fb374a546f78601a24ad"
    assert render["gpu_render_dispatched"] is True
    assert render["gpu_device_setdown_exception_code"] == 0xC0000005
    assert render["gpu_worker_classification"] == "nonzero_exit"
    assert render["gpu_output_rejected"] is True
    assert render["cpu_retry_new_process"] is True
    assert render["cpu_gpu_render_dispatched"] is False
    assert render["cpu_worker_classification"] == "ok"
    assert render["gpu_fallback_used"] is True
    assert render["guard_bytes_intact"] is True
    assert render["passed"] is True


def test_broker_removes_gpu_output_before_fresh_cpu_retry():
    source = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text()
    remove = source.index("fs::remove_file(&output_raw)")
    cpu_command = source.index("RenderGpuBackend::Cpu", remove)
    retry = source.index("isolated = dispatch_secure_image", cpu_command)
    assert remove < cpu_command < retry
