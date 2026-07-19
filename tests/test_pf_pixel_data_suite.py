import os
import re
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def source_text():
    # Suite structs, tables, and callbacks live in the worker-runtime owner
    # set; the "not in" checks below are scoped to regex slices of specific
    # functions, so the growable contract stays safe here.
    return source_owners.worker_text()


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_pf_pixel_data_suite_v1_v2_match_sdk_abi():
    text = source_text()
    assert "struct PixelDataSuite1" in text
    assert "struct PixelDataSuite2" in text
    assert "sizeof(PixelDataSuite1) == 3 * sizeof(void*)" in text
    assert "sizeof(PixelDataSuite2) == 4 * sizeof(void*)" in text
    assert "offsetof(PixelDataSuite2, get_pixel_data_float_gpu) == 3 * sizeof(void*)" in text
    assert re.search(
        r"PixelDataSuite1 g_pixel_data_suite1\{\s*&get_pixel_data8,\s*"
        r"&get_pixel_data16,\s*&get_pixel_data_float\}",
        text,
    )
    assert re.search(
        r"PixelDataSuite2 g_pixel_data_suite2\{\s*&get_pixel_data8,\s*"
        r"&get_pixel_data16,\s*&get_pixel_data_float,\s*"
        r"&get_pixel_data_float_gpu\}",
        text,
    )


def test_pf_pixel_data_suite_versions_are_acquirable_by_sdk_name():
    # The suite catalog entries stay in l2_main.
    text = (source_owners.L2_MAIN.read_text(encoding="utf-8") +
            (source_owners.SRC / "worker_host_suite_wiring.cpp").read_text(encoding="utf-8"))
    for version in (1, 2):
        assert f'{{"PF Pixel Data Suite", {version}, &g_pixel_data_suite{version}}}' in text


def test_pf_pixel_data_depths_use_registry_and_fail_closed():
    text = source_text()
    body = re.search(
        r"int32_t get_typed_pixel_data\(.*?\n\}", text, re.DOTALL
    ).group(0)
    assert "*output = nullptr;" in body
    assert body.index("*output = nullptr;") < body.index("if (!world) return 4;")
    assert "resolve_dispatch_world_format(world, resolved)" in body
    assert "format != required_format" in body
    assert "return 0;" in body
    assert "resolved.data" in body
    assert "std::abs(rowbytes) < width * pixel_bytes" in body
    expected = {
        "get_pixel_data8": ("kPixelFormatArgb32", "4"),
        "get_pixel_data16": ("kPixelFormatArgb64", "8"),
        "get_pixel_data_float": ("kPixelFormatArgb128", "16"),
        "get_pixel_data_float_gpu": ("kPixelFormatGpuBgra128", "16"),
    }
    for callback, (pixel_format, pixel_bytes) in expected.items():
        pattern = (
            rf"{callback}\(.*?\) \{{\s*return get_typed_pixel_data\("
            rf"world, .*?output, {pixel_format}, {pixel_bytes}\);"
        )
        assert re.search(pattern, text, re.DOTALL), callback


def test_pf_pixel_data_gpu_does_not_accept_cpu_float_worlds_by_alias():
    text = source_text()
    gpu = re.search(
        r"int32_t __cdecl get_pixel_data_float_gpu\([^)]*\) \{.*?\n\}", text, re.DOTALL
    ).group(0)
    assert "kPixelFormatGpuBgra128" in gpu
    assert "kPixelFormatArgb128" not in gpu
    assert "world, nullptr, output" in gpu


def test_pf_pixel_data_native_depth_and_registry_matrix():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-pixel-data"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_pixel_data_suite":"passed"}'
