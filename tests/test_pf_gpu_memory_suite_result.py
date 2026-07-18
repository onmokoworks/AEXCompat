import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_gpu_device_suite_memory_probe_is_balanced_and_rejects_double_free():
    result = json.loads(
        (ROOT / "analysis" / "PF_GPU_MEMORY_SUITE_RESULT_2026-07-15.json").read_text()
    )
    render = result["render"]
    assert result["fixture_sha256"] == "a8f5a20ae5f12eb4509b5a73fc7c21aee4b3530a080ef963b41adc1bcc9d62f0"
    assert render["input_sha256"] == render["output_sha256"]
    assert render["suite_acquires"] == render["suite_releases"] == 1
    assert render["allocations_created"] == render["allocations_freed"] == 2
    assert render["double_free_rejected"] is True
    assert render["invalid_operations"] == 1
    assert render["live_allocation_count"] == 0
    assert render["live_bytes"] == 0
    assert render["exclusive_access_depth"] == 0
    assert render["gpu_memory_lifetimes_balanced"] is True
    assert render["guard_bytes_intact"] is True
    assert render["passed"] is True


def test_gpu_device_suite_has_no_unsupported_slots_and_is_bounded():
    source = (ROOT / "minihost" / "src" / "gpu_memory_world_transport.cpp").read_text()
    table = source[source.index("std::array<void*, 15> gpu_device_suite1") :]
    table = table[: table.index("};")]
    assert "gpu_unsupported" not in table
    assert "kMaxGpuAllocations = 256" in source
    assert "kMaxGpuAllocationBytes = 256u * 1024u * 1024u" in source
    assert "gpu_memory_lifetimes_balanced()" in source
