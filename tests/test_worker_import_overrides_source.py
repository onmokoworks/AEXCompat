from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost/src/worker_import_overrides.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_import_overrides.cpp").read_text(
    encoding="utf-8"
)
ADMISSION = (ROOT / "minihost/src/worker_runtime_admission.cpp").read_text(
    encoding="utf-8"
)
L2_MAIN = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")
SELFTEST = (ROOT / "tests/native/worker_import_overrides_selftest.cpp").read_text(
    encoding="utf-8"
)


def test_deterministic_openmp_import_is_owned_by_one_bounded_component():
    for marker in (
        "kDeterministicOpenMpThreads = 1",
        "deterministic_omp_get_max_threads",
        "install_deterministic_import_overrides",
    ):
        assert marker in HEADER
    for marker in (
        'ascii_equal_folded(library, "vcomp140.dll")',
        'std::strcmp(name, "omp_get_max_threads") == 0',
        "VirtualProtect",
        "vcomp140 IAT protection restore failed",
        "IMAGE_SNAP_BY_ORDINAL64",
    ):
        assert marker in SOURCE


def test_every_executable_minihost_admission_path_installs_the_override():
    assert "install_deterministic_import_overrides" in ADMISSION
    assert "stage=import_overrides" in ADMISSION
    assert "install_deterministic_import_overrides" in L2_MAIN
    assert "stage:cluster_import_overrides" in L2_MAIN
    assert "src/worker_import_overrides.cpp" in CMAKE


def test_windows_selftest_drives_the_named_thunk_and_fail_closed_case():
    for marker in (
        "worker_import_overrides_selftest",
        "tests/native/worker_import_overrides_selftest.cpp",
    ):
        assert marker in CMAKE
    for marker in (
        '"VCOMP140.DLL"',
        '"omp_get_max_threads"',
        "function() == kDeterministicOpenMpThreads",
        "malformed_rejected",
    ):
        assert marker in SELFTEST
