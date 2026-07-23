import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sdk_gpu_failure_uses_a_fresh_cpu_worker_and_rejects_gpu_output():
    """Historical record: the Auto GPU->CPU retry this measured is removed.

    #365 (W4) deleted the one-shot argv transport, and the retry lived only
    there -- a resident session cannot re-dispatch mid-flight, so a GPU session
    failure now fails closed instead of re-rendering on the CPU and reporting
    `gpu_fallback_used`. The frozen JSON below stays as a record of what was
    observed when the retry existed; it no longer describes the shipped host.
    Retiring or re-labelling it is an evidence-corpus decision, tracked
    separately rather than made here.
    """
    result = json.loads(
        (ROOT / "analysis" / "SDK_GPU_CPU_FALLBACK_RESULT_2026-07-15.json").read_text(encoding="utf-8")
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
