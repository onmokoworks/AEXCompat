import os
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def source_text():
    return SOURCE.read_text(encoding="utf-8")


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_iterate_suite2_structs_match_sdk_abi_without_null_slots():
    text = source_text()
    assert "sizeof(Iterate8Suite2) == 5 * sizeof(void*)" in text
    assert "sizeof(Iterate16Suite2) == 3 * sizeof(void*)" in text
    assert "sizeof(IterateFloatSuite2) == 3 * sizeof(void*)" in text
    assert re.search(
        r"Iterate8Suite2 g_iterate8_suite2\{reinterpret_cast<void\*>\(&iterate_world8\), "
        r"&iterate_origin8, &iterate_lut8,\s*"
        r"&iterate_origin_non_clip8, &iterate_generic\}",
        text,
    )
    assert re.search(
        r"Iterate16Suite2 g_iterate16_suite2\{&iterate_world16, &iterate_origin16,\s*"
        r"&iterate_origin_non_clip16\}",
        text,
    )
    assert re.search(
        r"IterateFloatSuite2 g_iterate_float_suite2\{&iterate_world_float, &iterate_origin_float,\s*"
        r"&iterate_origin_non_clip_float\}",
        text,
    )


def test_iterate_callbacks_are_bounded_and_propagate_errors():
    text = source_text()
    assert "normalize_legacy_rect(area, std::min(source_width, destination_width)" in text
    assert "constexpr int32_t kMaxIterations = 16'777'216;" in text
    assert "iterations <= 0 || iterations > kMaxIterations" in text
    assert "if (error != 0) return error;" in text
    assert "tables[channel] ? tables[channel][value] : value" in text
    assert "std::array<unsigned char, 16> zero{};" in text


def test_world_iterate_reports_rows_and_checks_abort_without_masking_pixel_errors():
    text = source_text()
    assert "progress_span * completed_rows / rows" in text
    assert "progress_callback(effect_ref, current, progress_final)" in text
    assert "completed_rows < rows && abort_callback" in text
    assert "if (error != 0) return error;" in text
    assert "interaction.progress != std::vector<int32_t>({11, 12, 13, 14})" in text
    assert "interaction.pixel_calls != 2 || interaction.abort_calls != 2" in text
    assert "interaction.pixel_calls != 2 || interaction.abort_calls != 1" in text
    assert "pixel_bytes : {4, 8, 16}" in text


def test_iterate_suite_names_and_versions_are_acquirable():
    text = source_text()
    for name, global_name in (
        ("PF Iterate8 Suite", "g_iterate8_suite2"),
        ("PF iterate16 Suite", "g_iterate16_suite2"),
        ("PF iterateFloat Suite", "g_iterate_float_suite2"),
    ):
        assert re.search(
            rf'std::strcmp\(name, "{name}"\) == 0 &&\s*'
            r'\(version == 1 \|\| version == 2\)\) \{.*?'
            rf"\*suite = &{global_name};",
            text,
            re.DOTALL,
        )


def test_iterate_native_lut_non_clip_generic_and_error_paths():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-iterate"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_iterate_suite":"passed"}'
