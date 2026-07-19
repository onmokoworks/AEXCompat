import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
PF_SUITES_SOURCE = ROOT / "minihost" / "src" / "worker_pf_suites.cpp"
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
    end = source.index("StaticProviderCatalog component_catalog", start)
    return source[start:end]


def test_general_effect_suites_are_available_without_mask_mode_and_are_version_exact():
    source = SOURCE.read_text(encoding="utf-8")
    catalog = component_catalog_source(source)

    for name, (version, function_markers) in SUITES.items():
        entries = re.findall(rf'\{{"{re.escape(name)}",\s*(\d+),[^\n]*', catalog)
        assert entries == [str(version)], f"{name} must have one exact-version provider"
        for marker in function_markers:
            assert f"&{marker}" in source

    assert "StaticProviderCatalog component_catalog" in source
    assert "resolve_static_provider, &component_catalog" in source
    assert "acquire_host_suite(catalog, name, version, suite" in source


def test_general_effect_suite_functions_keep_existing_safety_bounds():
    source = SOURCE.read_text(encoding="utf-8") + PF_SUITES_SOURCE.read_text(encoding="utf-8") + WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")

    for marker in (
        "int32_t __cdecl subpixel_sample16(",
        "int32_t __cdecl subpixel_sample_float(",
        "int32_t __cdecl blend_world(",
        "int32_t __cdecl convolve_world(",
        "int32_t __cdecl copy_world8(",
        "int32_t __cdecl fill_world8(",
        "int32_t __cdecl fill_world16(",
        "int32_t __cdecl fill_world_float(",
        "kernel_size > 15",
        "width <= 4096 && height <= 4096",
        "static_cast<int64_t>(width) * height <= 16'777'216",
        "rowbytes <= 4096 * 16",
    ):
        assert marker in source


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
