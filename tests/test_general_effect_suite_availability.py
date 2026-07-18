import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
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


def acquire_suite_source(source: str) -> str:
    start = source.index("int32_t __cdecl acquire_suite(")
    end = source.index("int32_t __cdecl release_suite(", start)
    return source[start:end]


def suite_branch(acquire: str, name: str, version: int) -> str:
    pattern = re.compile(
        rf'if \((?P<condition>[^{{]*std::strcmp\(name, "{re.escape(name)}"\) == 0[^{{]*)\) '
        rf'\{{(?P<body>.*?)\n  \}}',
        re.DOTALL,
    )
    matches = [match for match in pattern.finditer(acquire) if f"version == {version}" in match["condition"]]
    assert len(matches) == 1, f"expected one {name} v{version} acquire branch"
    return matches[0]["condition"] + matches[0]["body"]


def test_general_effect_suites_are_available_without_mask_mode_and_are_version_exact():
    source = SOURCE.read_text(encoding="utf-8")
    acquire = acquire_suite_source(source)

    for name, (version, function_markers) in SUITES.items():
        branch = suite_branch(acquire, name, version)
        assert "g_mask_model_enabled" not in branch
        assert f"version == {version}" in branch
        assert "*suite =" in branch
        assert "record_suite_acquire(name, version)" in branch
        assert "return 0;" in branch
        for marker in function_markers:
            assert f"&{marker}" in branch

        versions = re.findall(
            rf'std::strcmp\(name, "{re.escape(name)}"\) == 0[^{{]*version == (\d+)', acquire
        )
        assert versions == [str(version)], f"{name} must not accept an uncontracted version"

    assert "return reject_suite_acquire(name, version);" in acquire
    assert "*suite = nullptr;" in acquire


def test_general_effect_suite_functions_keep_existing_safety_bounds():
    source = SOURCE.read_text(encoding="utf-8") + WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")

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
