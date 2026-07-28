from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost/src/worker_openmp_policy.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_openmp_policy.cpp").read_text(
    encoding="utf-8"
)
L1 = (ROOT / "minihost/src/main.cpp").read_text(encoding="utf-8")
L2 = source_owners.l2_translation_unit_text()
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")
SELFTEST = (ROOT / "tests/native/worker_openmp_policy_selftest.cpp").read_text(
    encoding="utf-8"
)


def test_policy_sets_one_process_wide_thread_count_before_plugin_loads():
    assert 'kThreadCountVariable[] = L"OMP_NUM_THREADS"' in HEADER
    assert 'kDeterministicThreadCount[] = L"1"' in HEADER
    assert "_wputenv_s(kThreadCountVariable, kDeterministicThreadCount)" in SOURCE
    assert "GetEnvironmentVariableW" in SOURCE
    assert L1.index("install_deterministic_policy") < L1.index("LoadLibraryExW")
    worker_entry = L2.split("int aexcompat::worker_target::run", 1)[1]
    assert worker_entry.index("install_deterministic_policy") < worker_entry.index(
        "worker_main_impl"
    )


def test_policy_is_linked_to_every_worker_and_exercised_against_vcomp():
    for marker in (
        "src/worker_openmp_policy.cpp",
        "worker_openmp_policy_selftest",
        "tests/native/worker_openmp_policy_selftest.cpp",
    ):
        assert marker in CMAKE
    for marker in (
        'LoadLibraryExW(',
        'L"vcomp140.dll"',
        'GetProcAddress(vcomp, "omp_get_max_threads")',
        "get_max_threads() == 1",
    ):
        assert marker in SELFTEST
