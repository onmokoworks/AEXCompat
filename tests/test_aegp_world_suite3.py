import os
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
ABI_SOURCE = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
ABI_OWNER = ROOT / "minihost" / "src" / "worker_suite_abi.cpp"


def _worker() -> pathlib.Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        pathlib.Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_world_suite3_is_exact_typed_sdk_layout():
    text = SOURCE.read_text(encoding="utf-8")
    abi = ABI_SOURCE.read_text(encoding="utf-8")
    owner = ABI_OWNER.read_text(encoding="utf-8")
    assert "struct AegpWorldSuite3" in abi
    assert "static_assert(sizeof(AegpWorldSuite3) == 13 * sizeof(void*));" in abi
    assert "offsetof(AegpWorldSuite3, reference_platform_world) == 12 * sizeof(void*)" in abi
    assert "AegpWorldSuite3 g_aegp_world_suite3" in owner
    assert "aegp_world_suite3_table()" in text
    assert "std::array<void*, 13> g_aegp_world_suite3" not in text
    assert "g_aegp_world_suite3.fill" not in text
    for callback in (
        "aegp_world_get_type",
        "aegp_world_get_size",
        "aegp_world_get_rowbytes",
        "aegp_world_get_base_addr8",
        "aegp_world_get_base_addr16",
        "aegp_world_get_base_addr32",
        "aegp_world_fill_pf_world",
    ):
        assert f"&{callback}" in text


def test_aegp_world_selftests_have_an_independent_translation_unit():
    owner = (ROOT / "minihost" / "src" / "worker_aegp_world_selftests.cpp").read_text(
        encoding="utf-8"
    )
    cmake = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")
    main = SOURCE.read_text(encoding="utf-8")
    assert "bool verify_world_suite3(const Hooks& h)" in owner
    assert "bool verify_world_mfr_safety(const Hooks& h)" in owner
    assert "bool verify_async_receipts(const Hooks& h)" in owner
    assert "class SyntheticReceiptScope" in owner
    assert "bool verify_aegp_world_suite3()" in main
    assert "std::atomic_bool start" not in main
    assert "worker_aegp_world_selftests.cpp" in cmake


def test_world_suite3_callbacks_are_cross_tu_calling_convention_checked():
    text = SOURCE.read_text(encoding="utf-8")
    abi = ABI_SOURCE.read_text(encoding="utf-8")
    world_callbacks = abi[abi.index("using AegpWorldNew"):
                          abi.index("struct AegpWorldSuite3")]
    assert world_callbacks.count("int32_t(__cdecl*)") == 13
    for callback_type in (
        "AegpWorldNew",
        "AegpWorldDispose",
        "AegpWorldGetType",
        "AegpWorldGetSize",
        "AegpWorldGetRowbytes",
        "AegpWorldGetBaseAddress8",
        "AegpWorldGetBaseAddress16",
        "AegpWorldGetBaseAddress32",
        "AegpWorldFillPfWorld",
        "AegpWorldFastBlur",
        "AegpWorldNewPlatform",
        "AegpWorldDisposePlatform",
        "AegpWorldReferencePlatform",
    ):
        assert f"aexcompat::suite_abi::{callback_type}>" in text


def test_world_suite3_runtime_matrix():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    result = subprocess.run(
        [str(worker), "--self-test-aegp-world-suite3"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    assert result.stdout.strip() == '{"aegp_world_suite3":"passed"}'
