import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
PF_SUITES_SOURCE = ROOT / "minihost" / "src" / "worker_pf_suites.cpp"
PF_SAMPLING_SOURCE = ROOT / "minihost" / "src" / "worker_pf_sampling_runtime.cpp"
PF_WORLD_TRANSFORM_SOURCE = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"
HOST_CATALOG_SOURCE = ROOT / "minihost" / "src" / "worker_host_suite_catalog.cpp"
RESULT = ROOT / "analysis" / "GENERAL_EFFECT_SUITE_AVAILABILITY_RESULT_2026-07-16.json"

SUITES = {
    "PF Sampling16 Suite": (1, ("subpixel_sample16",)),
    "PF SamplingFloat Suite": (1, ("subpixel_sample_float",)),
    "PF World Transform Suite": (
        1,
        ("blend_world", "convolve_world", "copy_world8", "transfer_rect", "transform_world"),
    ),
    "PF Fill Matte Suite": (2, ("fill_world8", "fill_world16", "fill_world_float")),
}

def component_catalog_source(source: str) -> str:
    start = source.index("const StaticSuite component_suites[]")
    end = source.index("return configure_host_suite_catalog", start)
    return source[start:end]

def test_result_is_truthful_source_contract_evidence():
    result = json.loads(RESULT.read_text(encoding="utf-8"))

    assert result["evidence_kind"] == "source_contract"
    assert result["runtime_measurement_performed"] is False
    assert result["measured_render_availability"] is None
    assert result["conclusion"] == "source_contract_passed_runtime_measurement_pending"
    assert result["contracted_suites"] == [
        {"name": name, "version": version} for name, (version, _) in SUITES.items()
    ]
    assert result["unknown_versions"] == "rejected_by_exact_match_and_fallback"
    assert result["safety_upper_bounds"] == {
        "maximum_width": 4096,
        "maximum_height": 4096,
        "maximum_pixels": 16_777_216,
        "maximum_rowbytes": 4096 * 16,
        "maximum_convolution_kernel_size": 15,
    }
